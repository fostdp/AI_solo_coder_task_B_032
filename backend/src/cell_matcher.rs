use crate::models::{BatteryGroup, CellInfo, CellGrade, GroupingAlgorithm, GroupingResult};
use chrono::Utc;
use rand::Rng;
use std::fmt;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MatcherConfig {
    pub cells_per_group: usize,
    pub max_capacity_diff: f64,
    pub max_resistance_diff: f64,
    pub min_capacity_ratio: f64,
    pub algorithm: GroupingAlgorithm,
    pub genetic_params: GeneticParams,
    pub channel_buffer: usize,
}

#[derive(Debug, Clone)]
pub struct GeneticParams {
    pub population_size: usize,
    pub max_generations: usize,
    pub mutation_rate: f64,
    pub crossover_rate: f64,
    pub elite_count: usize,
    pub time_limit_ms: u64,
    pub large_dataset_threshold: usize,
    pub fallback_to_greedy_on_timeout: bool,
}

impl Default for GeneticParams {
    fn default() -> Self {
        Self {
            population_size: 100,
            max_generations: 50,
            mutation_rate: 0.1,
            crossover_rate: 0.8,
            elite_count: 5,
            time_limit_ms: 30000,
            large_dataset_threshold: 1000,
            fallback_to_greedy_on_timeout: true,
        }
    }
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            cells_per_group: 16,
            max_capacity_diff: 0.05,
            max_resistance_diff: 1.0,
            min_capacity_ratio: 0.85,
            algorithm: GroupingAlgorithm::Genetic,
            genetic_params: GeneticParams::default(),
            channel_buffer: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchRequest {
    pub request_id: String,
    pub batch_id: String,
    pub cells: Vec<CellInfo>,
    pub cells_per_group: Option<usize>,
    pub algorithm: Option<GroupingAlgorithm>,
    pub max_capacity_diff: Option<f64>,
    pub max_resistance_diff: Option<f64>,
    pub respond_to: Option<oneshot::Sender<MatchResult>>,
}

#[derive(Debug)]
pub struct MatchResult {
    pub request_id: String,
    pub result: GroupingResult,
}

pub enum MatcherMessage {
    Match(MatchRequest),
    UpdateConfig(MatcherConfig),
    Shutdown,
}

impl fmt::Debug for MatcherMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatcherMessage::Match(req) => write!(f, "MatchRequest({})", req.request_id),
            MatcherMessage::UpdateConfig(_) => write!(f, "UpdateConfig"),
            MatcherMessage::Shutdown => write!(f, "Shutdown"),
        }
    }
}

pub type MatcherSender = mpsc::Sender<MatcherMessage>;
pub type MatcherReceiver = mpsc::Receiver<MatcherMessage>;

#[derive(Clone)]
pub struct CellMatcherHandle {
    sender: MatcherSender,
    config: Arc<Mutex<MatcherConfig>>,
}

impl CellMatcherHandle {
    pub fn new(sender: MatcherSender, config: MatcherConfig) -> Self {
        Self {
            sender,
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub async fn request_match(&self, request: MatchRequest) -> Result<oneshot::Receiver<MatchResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = MatcherMessage::Match(MatchRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send match request: {}", e))?;

        Ok(rx)
    }

    pub async fn update_config(&self, config: MatcherConfig) -> Result<(), String> {
        *self.config.lock().await = config.clone();
        self.sender
            .send(MatcherMessage::UpdateConfig(config))
            .await
            .map_err(|e| format!("Failed to send config update: {}", e))
    }

    pub async fn get_config(&self) -> MatcherConfig {
        self.config.lock().await.clone()
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.sender
            .send(MatcherMessage::Shutdown)
            .await
            .map_err(|e| format!("Failed to send shutdown: {}", e))
    }
}

pub struct CellMatcherService {
    config: MatcherConfig,
    receiver: MatcherReceiver,
    active_requests: usize,
}

impl CellMatcherService {
    pub fn new(config: MatcherConfig) -> (Self, CellMatcherHandle) {
        let (sender, receiver) = mpsc::channel(config.channel_buffer);
        let handle = CellMatcherHandle::new(sender, config.clone());
        (
            Self {
                config,
                receiver,
                active_requests: 0,
            },
            handle,
        )
    }

    pub async fn run(mut self) {
        tracing::info!("CellMatcherService started, waiting for requests");

        while let Some(message) = self.receiver.recv().await {
            match message {
                MatcherMessage::Match(request) => {
                    self.active_requests += 1;
                    tracing::debug!("Processing match request: {}", request.request_id);

                    let config = self.build_effective_config(&request);
                    let request_id = request.request_id.clone();
                    let respond_to = request.respond_to;

                    let result = tokio::task::spawn_blocking(move || {
                        process_match_request_sync(request, config)
                    })
                    .await;

                    let match_result = match result {
                        Ok(result) => MatchResult {
                            request_id: request_id.clone(),
                            result,
                        },
                        Err(e) => {
                            tracing::error!("Match task panicked: {}", e);
                            MatchResult {
                                request_id: request_id.clone(),
                                result: GroupingResult {
                                    batch_id: request_id,
                                    algorithm: GroupingAlgorithm::Genetic,
                                    total_cells: 0,
                                    rejected_cells: 0,
                                    group_count: 0,
                                    cells_per_group: 0,
                                    groups: Vec::new(),
                                    avg_consistency_score: 0.0,
                                    processing_time_ms: 0,
                                },
                            }
                        }
                    };

                    if let Some(tx) = respond_to {
                        let _ = tx.send(match_result);
                    }

                    self.active_requests -= 1;
                    tracing::debug!("Completed match request: {}", request_id);
                }
                MatcherMessage::UpdateConfig(new_config) => {
                    self.config = new_config;
                    tracing::info!("Matcher config updated");
                }
                MatcherMessage::Shutdown => {
                    tracing::info!("CellMatcherService shutting down");
                    break;
                }
            }
        }

        tracing::info!("CellMatcherService stopped");
    }

    fn build_effective_config(&self, request: &MatchRequest) -> MatcherConfig {
        let mut config = self.config.clone();

        if let Some(cpg) = request.cells_per_group {
            config.cells_per_group = cpg;
        }
        if let Some(algo) = request.algorithm {
            config.algorithm = algo;
        }
        if let Some(cap_diff) = request.max_capacity_diff {
            config.max_capacity_diff = cap_diff;
        }
        if let Some(res_diff) = request.max_resistance_diff {
            config.max_resistance_diff = res_diff;
        }

        config
    }

    pub fn active_requests(&self) -> usize {
        self.active_requests
    }
}

fn process_match_request_sync(request: MatchRequest, config: MatcherConfig) -> GroupingResult {
    let start_time = std::time::Instant::now();
    let total_cells = request.cells.len();

    let valid_cells: Vec<CellInfo> = request
        .cells
        .into_iter()
        .filter(|c| c.capacity_ratio >= config.min_capacity_ratio)
        .collect();

    let rejected_cells = total_cells - valid_cells.len();

    if valid_cells.len() < config.cells_per_group {
        return GroupingResult {
            batch_id: request.batch_id.clone(),
            algorithm: config.algorithm,
            total_cells,
            rejected_cells,
            group_count: 0,
            cells_per_group: config.cells_per_group,
            groups: Vec::new(),
            avg_consistency_score: 0.0,
            processing_time_ms: start_time.elapsed().as_millis() as u64,
        };
    }

    let groups = match config.algorithm {
        GroupingAlgorithm::Greedy => greedy_grouping(&config, valid_cells, &request.batch_id),
        GroupingAlgorithm::Genetic => genetic_grouping(&config, valid_cells, &request.batch_id),
    };

    let avg_consistency_score = if groups.is_empty() {
        0.0
    } else {
        groups.iter().map(|g| g.consistency_score).sum::<f64>() / groups.len() as f64
    };

    GroupingResult {
        batch_id: request.batch_id,
        algorithm: config.algorithm,
        total_cells,
        rejected_cells,
        group_count: groups.len(),
        cells_per_group: config.cells_per_group,
        groups,
        avg_consistency_score,
        processing_time_ms: start_time.elapsed().as_millis() as u64,
    }
}

fn greedy_grouping(config: &MatcherConfig, mut cells: Vec<CellInfo>, batch_id: &str) -> Vec<BatteryGroup> {
    cells.sort_by(|a, b| b.capacity_ratio.partial_cmp(&a.capacity_ratio).unwrap());

    let mut groups = Vec::new();
    let mut group_number = 0;

    while cells.len() >= config.cells_per_group {
        group_number += 1;
        let seed = cells.remove(0);
        let mut group_cells = vec![seed.clone()];

        let mut i = 0;
        while group_cells.len() < config.cells_per_group && i < cells.len() {
            let candidate = &cells[i];

            let group_caps: Vec<f64> = group_cells.iter().map(|c| c.measured_capacity).collect();
            let group_res: Vec<f64> = group_cells.iter().map(|c| c.internal_resistance).collect();

            let max_cap = group_caps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_cap = group_caps.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_res = group_res.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_res = group_res.iter().cloned().fold(f64::INFINITY, f64::min);

            let cap_diff_ok = (candidate.measured_capacity - max_cap).abs() < config.max_capacity_diff
                && (candidate.measured_capacity - min_cap).abs() < config.max_capacity_diff;
            let res_diff_ok = (candidate.internal_resistance - max_res).abs() < config.max_resistance_diff
                && (candidate.internal_resistance - min_res).abs() < config.max_resistance_diff;

            if cap_diff_ok && res_diff_ok {
                group_cells.push(cells.remove(i));
            } else {
                i += 1;
            }
        }

        if group_cells.len() == config.cells_per_group {
            groups.push(create_battery_group(config, group_cells, batch_id, group_number));
        } else {
            cells.extend(group_cells.into_iter().skip(1));
        }
    }

    groups
}

fn genetic_grouping(config: &MatcherConfig, cells: Vec<CellInfo>, batch_id: &str) -> Vec<BatteryGroup> {
    let num_groups = cells.len() / config.cells_per_group;
    if num_groups == 0 {
        return Vec::new();
    }

    let params = &config.genetic_params;
    let start_time = std::time::Instant::now();

    if cells.len() >= params.large_dataset_threshold {
        return genetic_grouping_divide_conquer(config, cells, batch_id, num_groups);
    }

    let mut rng = rand::thread_rng();

    let mut population: Vec<Vec<usize>> = (0..params.population_size)
        .map(|_| {
            let mut permutation: Vec<usize> = (0..cells.len()).collect();
            for i in (1..permutation.len()).rev() {
                let j = rng.gen_range(0..=i);
                permutation.swap(i, j);
            }
            permutation
        })
        .collect();

    let mut best_fitness = f64::NEG_INFINITY;
    let mut best_individual: Option<Vec<usize>> = None;
    let mut timed_out = false;

    for _generation in 0..params.max_generations {
        if start_time.elapsed().as_millis() as u64 >= params.time_limit_ms {
            timed_out = true;
            break;
        }

        let fitnesses: Vec<f64> = population
            .iter()
            .map(|ind| fitness_function(config, ind, &cells, num_groups))
            .collect();

        for (i, &fitness) in fitnesses.iter().enumerate() {
            if fitness > best_fitness {
                best_fitness = fitness;
                best_individual = Some(population[i].clone());
            }
        }

        let mut new_population = Vec::new();

        let mut elite_indices: Vec<usize> = (0..population.len()).collect();
        elite_indices.sort_by(|&a, &b| fitnesses[b].partial_cmp(&fitnesses[a]).unwrap());
        for &idx in elite_indices.iter().take(params.elite_count) {
            new_population.push(population[idx].clone());
        }

        while new_population.len() < params.population_size {
            let parent1 = tournament_selection(&population, &fitnesses, 3, &mut rng);
            let parent2 = tournament_selection(&population, &fitnesses, 3, &mut rng);

            let child = if rng.gen::<f64>() < params.crossover_rate {
                order_crossover(&population[parent1], &population[parent2], &mut rng)
            } else {
                population[parent1].clone()
            };

            let child = if rng.gen::<f64>() < params.mutation_rate {
                swap_mutation(&child, &mut rng)
            } else {
                child
            };

            new_population.push(child);
        }

        population = new_population;
    }

    if timed_out && best_individual.is_none() && params.fallback_to_greedy_on_timeout {
        return greedy_grouping(config, cells, batch_id);
    }

    match best_individual {
        Some(ind) => decode_individual(config, &ind, &cells, batch_id, num_groups),
        None => greedy_grouping(config, cells, batch_id),
    }
}

fn genetic_grouping_divide_conquer(
    config: &MatcherConfig,
    mut cells: Vec<CellInfo>,
    batch_id: &str,
    num_groups: usize,
) -> Vec<BatteryGroup> {
    let _ = num_groups;
    let params = &config.genetic_params;
    let start_time = std::time::Instant::now();

    cells.sort_by(|a, b| a.measured_capacity.partial_cmp(&b.measured_capacity).unwrap());

    let chunk_size = (cells.len() / 4).max(params.large_dataset_threshold);
    let mut all_groups = Vec::new();
    let mut chunk_offset = 0;

    for (chunk_idx, chunk) in cells.chunks(chunk_size).enumerate() {
        if start_time.elapsed().as_millis() as u64 >= params.time_limit_ms {
            let remaining: Vec<CellInfo> = cells.chunks(chunk_size).skip(chunk_idx)
                .flatten().cloned().collect();
            let greedy_groups = greedy_grouping_with_offset(
                config, remaining, batch_id, all_groups.len() as u32, chunk_offset
            );
            all_groups.extend(greedy_groups);
            break;
        }

        let chunk_cells: Vec<CellInfo> = chunk.to_vec();
        let chunk_num_groups = chunk_cells.len() / config.cells_per_group;
        
        if chunk_num_groups > 0 {
            let sub_params = GeneticParams {
                population_size: params.population_size / 2,
                max_generations: params.max_generations / 2,
                time_limit_ms: params.time_limit_ms / 4,
                ..params.clone()
            };
            
            let sub_config = MatcherConfig {
                genetic_params: sub_params,
                ..config.clone()
            };
            
            let sub_groups = genetic_grouping_standard(
                &sub_config, chunk_cells, batch_id, all_groups.len() as u32, chunk_offset
            );
            
            let grouped_count = sub_groups.len() * config.cells_per_group;
            chunk_offset += grouped_count;
            all_groups.extend(sub_groups);
        }
    }

    all_groups
}

fn genetic_grouping_standard(
    config: &MatcherConfig,
    cells: Vec<CellInfo>,
    batch_id: &str,
    group_number_offset: u32,
    cell_id_offset: usize,
) -> Vec<BatteryGroup> {
    let num_groups = cells.len() / config.cells_per_group;
    if num_groups == 0 {
        return Vec::new();
    }

    let params = &config.genetic_params;
    let start_time = std::time::Instant::now();
    let mut rng = rand::thread_rng();

    let mut population: Vec<Vec<usize>> = (0..params.population_size)
        .map(|_| {
            let mut permutation: Vec<usize> = (0..cells.len()).collect();
            for i in (1..permutation.len()).rev() {
                let j = rng.gen_range(0..=i);
                permutation.swap(i, j);
            }
            permutation
        })
        .collect();

    let mut best_fitness = f64::NEG_INFINITY;
    let mut best_individual: Option<Vec<usize>> = None;

    for _generation in 0..params.max_generations {
        if start_time.elapsed().as_millis() as u64 >= params.time_limit_ms {
            break;
        }

        let fitnesses: Vec<f64> = population
            .iter()
            .map(|ind| fitness_function(config, ind, &cells, num_groups))
            .collect();

        for (i, &fitness) in fitnesses.iter().enumerate() {
            if fitness > best_fitness {
                best_fitness = fitness;
                best_individual = Some(population[i].clone());
            }
        }

        let mut new_population = Vec::new();

        let mut elite_indices: Vec<usize> = (0..population.len()).collect();
        elite_indices.sort_by(|&a, &b| fitnesses[b].partial_cmp(&fitnesses[a]).unwrap());
        for &idx in elite_indices.iter().take(params.elite_count) {
            new_population.push(population[idx].clone());
        }

        while new_population.len() < params.population_size {
            let parent1 = tournament_selection(&population, &fitnesses, 3, &mut rng);
            let parent2 = tournament_selection(&population, &fitnesses, 3, &mut rng);

            let child = if rng.gen::<f64>() < params.crossover_rate {
                order_crossover(&population[parent1], &population[parent2], &mut rng)
            } else {
                population[parent1].clone()
            };

            let child = if rng.gen::<f64>() < params.mutation_rate {
                swap_mutation(&child, &mut rng)
            } else {
                child
            };

            new_population.push(child);
        }

        population = new_population;
    }

    match best_individual {
        Some(ind) => decode_individual_with_offset(
            config, &ind, &cells, batch_id, num_groups, group_number_offset, cell_id_offset
        ),
        None => greedy_grouping_with_offset(config, cells, batch_id, group_number_offset, cell_id_offset),
    }
}

fn greedy_grouping_with_offset(
    config: &MatcherConfig,
    mut cells: Vec<CellInfo>,
    batch_id: &str,
    group_number_offset: u32,
    _cell_id_offset: usize,
) -> Vec<BatteryGroup> {
    cells.sort_by(|a, b| b.capacity_ratio.partial_cmp(&a.capacity_ratio).unwrap());

    let mut groups = Vec::new();
    let mut group_number = group_number_offset;

    while cells.len() >= config.cells_per_group {
        group_number += 1;
        let seed = cells.remove(0);
        let mut group_cells = vec![seed.clone()];

        let mut i = 0;
        while group_cells.len() < config.cells_per_group && i < cells.len() {
            let candidate = &cells[i];

            let group_caps: Vec<f64> = group_cells.iter().map(|c| c.measured_capacity).collect();
            let group_res: Vec<f64> = group_cells.iter().map(|c| c.internal_resistance).collect();

            let max_cap = group_caps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_cap = group_caps.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_res = group_res.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_res = group_res.iter().cloned().fold(f64::INFINITY, f64::min);

            let cap_diff_ok = (candidate.measured_capacity - max_cap).abs() < config.max_capacity_diff
                && (candidate.measured_capacity - min_cap).abs() < config.max_capacity_diff;
            let res_diff_ok = (candidate.internal_resistance - max_res).abs() < config.max_resistance_diff
                && (candidate.internal_resistance - min_res).abs() < config.max_resistance_diff;

            if cap_diff_ok && res_diff_ok {
                group_cells.push(cells.remove(i));
            } else {
                i += 1;
            }
        }

        if group_cells.len() == config.cells_per_group {
            groups.push(create_battery_group(config, group_cells, batch_id, group_number));
        } else {
            cells.extend(group_cells.into_iter().skip(1));
        }
    }

    groups
}

fn decode_individual_with_offset(
    config: &MatcherConfig,
    individual: &[usize],
    cells: &[CellInfo],
    batch_id: &str,
    num_groups: usize,
    group_number_offset: u32,
    _cell_id_offset: usize,
) -> Vec<BatteryGroup> {
    let mut groups = Vec::new();

    for group_idx in 0..num_groups {
        let start = group_idx * config.cells_per_group;
        let end = start + config.cells_per_group;

        let group_cells: Vec<CellInfo> = (start..end)
            .map(|i| cells[individual[i]].clone())
            .collect();

        groups.push(create_battery_group(
            config,
            group_cells,
            batch_id,
            group_number_offset + group_idx as u32 + 1,
        ));
    }

    groups
}

fn fitness_function(config: &MatcherConfig, individual: &[usize], cells: &[CellInfo], num_groups: usize) -> f64 {
    let mut total_score = 0.0;

    for group_idx in 0..num_groups {
        let start = group_idx * config.cells_per_group;
        let end = start + config.cells_per_group;
        if end > cells.len() {
            break;
        }

        let group_cells: Vec<&CellInfo> = individual[start..end]
            .iter()
            .filter_map(|&idx| cells.get(idx))
            .collect();

        if group_cells.len() < config.cells_per_group {
            continue;
        }

        let score = calculate_group_consistency(config, &group_cells);
        total_score += score;
    }

    total_score
}

fn calculate_group_consistency(config: &MatcherConfig, cells: &[&CellInfo]) -> f64 {
    if cells.len() < 2 {
        return 0.0;
    }

    let capacities: Vec<f64> = cells.iter().map(|c| c.measured_capacity).collect();
    let resistances: Vec<f64> = cells.iter().map(|c| c.internal_resistance).collect();

    let cap_mean = capacities.iter().sum::<f64>() / capacities.len() as f64;
    let cap_std = calculate_std(&capacities, cap_mean);
    let cap_max_diff = capacities.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - capacities.iter().cloned().fold(f64::INFINITY, f64::min);

    let res_mean = resistances.iter().sum::<f64>() / resistances.len() as f64;
    let res_std = calculate_std(&resistances, res_mean);
    let res_max_diff = resistances.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - resistances.iter().cloned().fold(f64::INFINITY, f64::min);

    let cap_cv = cap_std / cap_mean;
    let res_cv = res_std / res_mean;

    let cap_diff_penalty = if cap_max_diff > config.max_capacity_diff {
        (cap_max_diff - config.max_capacity_diff) * 100.0
    } else {
        0.0
    };

    let res_diff_penalty = if res_max_diff > config.max_resistance_diff {
        (res_max_diff - config.max_resistance_diff) * 10.0
    } else {
        0.0
    };

    let consistency = 100.0 * (1.0 - cap_cv * 10.0) * (1.0 - res_cv * 10.0)
        - cap_diff_penalty
        - res_diff_penalty;

    consistency.max(0.0)
}

fn calculate_std(values: &[f64], mean: f64) -> f64 {
    let variance: f64 = values
        .iter()
        .map(|v| (v - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn tournament_selection(
    population: &[Vec<usize>],
    fitnesses: &[f64],
    tournament_size: usize,
    rng: &mut impl Rng,
) -> usize {
    let mut best_idx = rng.gen_range(0..population.len());
    for _ in 1..tournament_size {
        let idx = rng.gen_range(0..population.len());
        if fitnesses[idx] > fitnesses[best_idx] {
            best_idx = idx;
        }
    }
    best_idx
}

fn order_crossover(parent1: &[usize], parent2: &[usize], rng: &mut impl Rng) -> Vec<usize> {
    let len = parent1.len();
    let start = rng.gen_range(0..len);
    let end = rng.gen_range(start..len);

    let mut child = vec![usize::MAX; len];
    for i in start..=end {
        child[i] = parent1[i];
    }

    let mut parent2_iter = parent2.iter().filter(|&&x| !child[start..=end].contains(&x));
    for i in 0..len {
        if child[i] == usize::MAX {
            child[i] = *parent2_iter.next().unwrap();
        }
    }

    child
}

fn swap_mutation(individual: &[usize], rng: &mut impl Rng) -> Vec<usize> {
    let mut child = individual.to_vec();
    let idx1 = rng.gen_range(0..child.len());
    let idx2 = rng.gen_range(0..child.len());
    child.swap(idx1, idx2);
    child
}

fn decode_individual(
    config: &MatcherConfig,
    individual: &[usize],
    cells: &[CellInfo],
    batch_id: &str,
    num_groups: usize,
) -> Vec<BatteryGroup> {
    let mut groups = Vec::new();

    for group_idx in 0..num_groups {
        let start = group_idx * config.cells_per_group;
        let end = start + config.cells_per_group;
        if end > cells.len() {
            break;
        }

        let group_cells: Vec<CellInfo> = individual[start..end]
            .iter()
            .filter_map(|&idx| cells.get(idx).cloned())
            .collect();

        if group_cells.len() == config.cells_per_group {
            groups.push(create_battery_group(config, group_cells, batch_id, (group_idx + 1) as u32));
        }
    }

    groups
}

fn create_battery_group(config: &MatcherConfig, cells: Vec<CellInfo>, batch_id: &str, group_number: u32) -> BatteryGroup {
    let capacities: Vec<f64> = cells.iter().map(|c| c.measured_capacity).collect();
    let resistances: Vec<f64> = cells.iter().map(|c| c.internal_resistance).collect();

    let avg_capacity = capacities.iter().sum::<f64>() / capacities.len() as f64;
    let capacity_std = calculate_std(&capacities, avg_capacity);
    let capacity_max_diff = capacities.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - capacities.iter().cloned().fold(f64::INFINITY, f64::min);

    let avg_resistance = resistances.iter().sum::<f64>() / resistances.len() as f64;
    let resistance_std = calculate_std(&resistances, avg_resistance);
    let resistance_max_diff = resistances.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - resistances.iter().cloned().fold(f64::INFINITY, f64::min);

    let cell_refs: Vec<&CellInfo> = cells.iter().collect();
    let consistency_score = calculate_group_consistency(config, &cell_refs);

    let cell_ids: Vec<(u16, u32)> = cells
        .iter()
        .map(|c| (c.cabinet_id, c.channel_id))
        .collect();

    let group_id = Uuid::new_v4().to_string();

    BatteryGroup {
        date: Utc::now().date_naive(),
        group_id,
        batch_id: batch_id.to_string(),
        group_number,
        algorithm: config.algorithm,
        cell_count: cells.len() as u16,
        avg_capacity,
        capacity_std,
        capacity_max_diff,
        avg_resistance,
        resistance_std,
        resistance_max_diff,
        consistency_score,
        cell_ids,
    }
}

pub fn generate_cell_info(
    cabinet_id: u16,
    channel_id: u32,
    batch_id: String,
    predicted_capacity: f64,
    measured_capacity: f64,
    internal_resistance: f64,
    cycle_index: u16,
) -> CellInfo {
    let capacity_ratio = measured_capacity / crate::models::RATED_CAPACITY;
    let grade = CellGrade::from_capacity_ratio(capacity_ratio);

    CellInfo {
        date: Utc::now().date_naive(),
        batch_id,
        cabinet_id,
        channel_id,
        predicted_capacity,
        measured_capacity,
        internal_resistance,
        capacity_ratio,
        grade,
        cycle_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RATED_CAPACITY;

    fn generate_test_cells(count: usize, capacity_std: f64, resistance_std: f64) -> Vec<CellInfo> {
        let mut rng = rand::thread_rng();
        (0..count)
            .map(|i| {
                let measured_capacity = RATED_CAPACITY * (1.0 + rng.gen_range(-capacity_std..capacity_std));
                let internal_resistance = 20.0 + rng.gen_range(-resistance_std..resistance_std);
                generate_cell_info(
                    (i / 512) as u16,
                    (i % 512) as u32,
                    "TEST-BATCH-001".to_string(),
                    measured_capacity * 0.98,
                    measured_capacity,
                    internal_resistance,
                    1,
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn test_async_match_request() {
        let config = MatcherConfig {
            cells_per_group: 10,
            max_capacity_diff: 0.1,
            max_resistance_diff: 5.0,
            ..MatcherConfig::default()
        };

        let (service, handle) = CellMatcherService::new(config);

        tokio::spawn(service.run());

        let cells = generate_test_cells(100, 0.03, 1.0);
        let request = MatchRequest {
            request_id: "test-001".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            cells,
            cells_per_group: None,
            algorithm: Some(GroupingAlgorithm::Greedy),
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let rx = handle.request_match(request).await.unwrap();
        let result = rx.await.unwrap();

        assert_eq!(result.request_id, "test-001");
        assert!(result.result.group_count > 0);
        assert!(result.result.avg_consistency_score > 0.0);
    }

    #[tokio::test]
    async fn test_genetic_algorithm_async() {
        let config = MatcherConfig {
            cells_per_group: 10,
            max_capacity_diff: 0.1,
            max_resistance_diff: 5.0,
            genetic_params: GeneticParams {
                population_size: 30,
                max_generations: 15,
                ..GeneticParams::default()
            },
            ..MatcherConfig::default()
        };

        let (service, handle) = CellMatcherService::new(config);
        tokio::spawn(service.run());

        let cells = generate_test_cells(50, 0.03, 1.5);
        let request = MatchRequest {
            request_id: "test-genetic-001".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            cells,
            cells_per_group: None,
            algorithm: Some(GroupingAlgorithm::Genetic),
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let rx = handle.request_match(request).await.unwrap();
        let result = rx.await.unwrap();

        assert_eq!(result.request_id, "test-genetic-001");
        assert!(result.result.group_count >= 4);
        assert!(result.result.avg_consistency_score > 60.0);
    }

    #[tokio::test]
    async fn test_concurrent_match_requests() {
        let config = MatcherConfig {
            cells_per_group: 10,
            ..MatcherConfig::default()
        };

        let (service, handle) = CellMatcherService::new(config);
        tokio::spawn(service.run());

        let cells1 = generate_test_cells(30, 0.02, 1.0);
        let cells2 = generate_test_cells(40, 0.03, 1.5);

        let request1 = MatchRequest {
            request_id: "concurrent-1".to_string(),
            batch_id: "BATCH-1".to_string(),
            cells: cells1,
            cells_per_group: None,
            algorithm: Some(GroupingAlgorithm::Greedy),
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let request2 = MatchRequest {
            request_id: "concurrent-2".to_string(),
            batch_id: "BATCH-2".to_string(),
            cells: cells2,
            cells_per_group: None,
            algorithm: Some(GroupingAlgorithm::Greedy),
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let rx1 = handle.request_match(request1).await.unwrap();
        let rx2 = handle.request_match(request2).await.unwrap();

        let (result1, result2) = tokio::join!(rx1, rx2);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap().request_id, "concurrent-1");
        assert_eq!(result2.unwrap().request_id, "concurrent-2");
    }

    #[tokio::test]
    async fn test_config_update() {
        let config = MatcherConfig {
            cells_per_group: 10,
            ..MatcherConfig::default()
        };

        let (service, handle) = CellMatcherService::new(config);
        tokio::spawn(service.run());

        let new_config = MatcherConfig {
            cells_per_group: 20,
            max_capacity_diff: 0.03,
            ..MatcherConfig::default()
        };

        handle.update_config(new_config.clone()).await.unwrap();

        let current_config = handle.get_config().await;
        assert_eq!(current_config.cells_per_group, 20);
        assert_eq!(current_config.max_capacity_diff, 0.03);
    }

    #[test]
    fn test_genetic_algorithm_minimizes_capacity_diff_sync() {
        let cells = generate_test_cells(100, 0.03, 1.0);

        let config = MatcherConfig {
            cells_per_group: 10,
            max_capacity_diff: 0.1,
            max_resistance_diff: 5.0,
            min_capacity_ratio: 0.85,
            algorithm: GroupingAlgorithm::Genetic,
            genetic_params: GeneticParams {
                population_size: 50,
                max_generations: 30,
                mutation_rate: 0.1,
                crossover_rate: 0.8,
                elite_count: 3,
                ..GeneticParams::default()
            },
            channel_buffer: 100,
        };

        let request = MatchRequest {
            request_id: "sync-test-001".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            cells,
            cells_per_group: None,
            algorithm: None,
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let result = process_match_request_sync(request, config);

        assert!(result.group_count > 0, "Should create at least one group");

        for group in &result.groups {
            assert!(group.capacity_max_diff < 0.1,
                "Capacity max diff {:.4} should be < 0.1", group.capacity_max_diff);
            assert!(group.capacity_std < 0.03,
                "Capacity std {:.4} should be < 0.03", group.capacity_std);
        }
    }

    #[test]
    fn test_genetic_vs_greedy_comparison_sync() {
        let cells = generate_test_cells(100, 0.03, 1.5);

        let genetic_config = MatcherConfig {
            algorithm: GroupingAlgorithm::Genetic,
            ..MatcherConfig::default()
        };
        let greedy_config = MatcherConfig {
            algorithm: GroupingAlgorithm::Greedy,
            ..MatcherConfig::default()
        };

        let request_genetic = MatchRequest {
            request_id: "comp-genetic".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            cells: cells.clone(),
            cells_per_group: None,
            algorithm: None,
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let request_greedy = MatchRequest {
            request_id: "comp-greedy".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            cells,
            cells_per_group: None,
            algorithm: None,
            max_capacity_diff: None,
            max_resistance_diff: None,
            respond_to: None,
        };

        let genetic_result = process_match_request_sync(request_genetic, genetic_config);
        let greedy_result = process_match_request_sync(request_greedy, greedy_config);

        assert!(genetic_result.avg_consistency_score >= greedy_result.avg_consistency_score * 0.95,
            "Genetic avg consistency {:.2} should be >= greedy {:.2} * 0.95",
            genetic_result.avg_consistency_score, greedy_result.avg_consistency_score);
    }
}
