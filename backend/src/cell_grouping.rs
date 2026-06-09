use crate::models::{BatteryGroup, CellInfo, CellGrade, GroupingAlgorithm, GroupingResult};
use chrono::Utc;
use rand::Rng;
use std::f64::consts::E;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GroupingConfig {
    pub cells_per_group: usize,
    pub max_capacity_diff: f64,
    pub max_resistance_diff: f64,
    pub min_capacity_ratio: f64,
    pub algorithm: GroupingAlgorithm,
    pub genetic_params: GeneticParams,
}

#[derive(Debug, Clone)]
pub struct GeneticParams {
    pub population_size: usize,
    pub max_generations: usize,
    pub mutation_rate: f64,
    pub crossover_rate: f64,
    pub elite_count: usize,
}

impl Default for GeneticParams {
    fn default() -> Self {
        Self {
            population_size: 100,
            max_generations: 50,
            mutation_rate: 0.1,
            crossover_rate: 0.8,
            elite_count: 5,
        }
    }
}

impl Default for GroupingConfig {
    fn default() -> Self {
        Self {
            cells_per_group: 16,
            max_capacity_diff: 0.05,
            max_resistance_diff: 1.0,
            min_capacity_ratio: 0.85,
            algorithm: GroupingAlgorithm::Genetic,
            genetic_params: GeneticParams::default(),
        }
    }
}

pub struct CellGroupingService {
    config: GroupingConfig,
}

impl CellGroupingService {
    pub fn new(config: GroupingConfig) -> Self {
        Self { config }
    }

    pub fn group_cells(&self, cells: Vec<CellInfo>, batch_id: String) -> GroupingResult {
        let start_time = std::time::Instant::now();
        let total_cells = cells.len();

        let valid_cells: Vec<CellInfo> = cells
            .into_iter()
            .filter(|c| c.capacity_ratio >= self.config.min_capacity_ratio)
            .collect();

        let rejected_cells = total_cells - valid_cells.len();

        if valid_cells.len() < self.config.cells_per_group {
            return GroupingResult {
                batch_id,
                algorithm: self.config.algorithm,
                total_cells,
                rejected_cells,
                group_count: 0,
                cells_per_group: self.config.cells_per_group,
                groups: Vec::new(),
                avg_consistency_score: 0.0,
                processing_time_ms: start_time.elapsed().as_millis() as u64,
            };
        }

        let groups = match self.config.algorithm {
            GroupingAlgorithm::Greedy => self.greedy_grouping(valid_cells, &batch_id),
            GroupingAlgorithm::Genetic => self.genetic_grouping(valid_cells, &batch_id),
        };

        let avg_consistency_score = if groups.is_empty() {
            0.0
        } else {
            groups.iter().map(|g| g.consistency_score).sum::<f64>() / groups.len() as f64
        };

        GroupingResult {
            batch_id,
            algorithm: self.config.algorithm,
            total_cells,
            rejected_cells,
            group_count: groups.len(),
            cells_per_group: self.config.cells_per_group,
            groups,
            avg_consistency_score,
            processing_time_ms: start_time.elapsed().as_millis() as u64,
        }
    }

    fn greedy_grouping(&self, mut cells: Vec<CellInfo>, batch_id: &str) -> Vec<BatteryGroup> {
        cells.sort_by(|a, b| b.capacity_ratio.partial_cmp(&a.capacity_ratio).unwrap());

        let mut groups = Vec::new();
        let mut group_number = 0;

        while cells.len() >= self.config.cells_per_group {
            group_number += 1;
            let seed = cells.remove(0);
            let mut group_cells = vec![seed.clone()];

            let mut i = 0;
            while group_cells.len() < self.config.cells_per_group && i < cells.len() {
                let candidate = &cells[i];

                let group_caps: Vec<f64> = group_cells.iter().map(|c| c.measured_capacity).collect();
                let group_res: Vec<f64> = group_cells.iter().map(|c| c.internal_resistance).collect();

                let max_cap = group_caps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let min_cap = group_caps.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_res = group_res.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let min_res = group_res.iter().cloned().fold(f64::INFINITY, f64::min);

                let cap_diff_ok = (candidate.measured_capacity - max_cap).abs() < self.config.max_capacity_diff
                    && (candidate.measured_capacity - min_cap).abs() < self.config.max_capacity_diff;
                let res_diff_ok = (candidate.internal_resistance - max_res).abs() < self.config.max_resistance_diff
                    && (candidate.internal_resistance - min_res).abs() < self.config.max_resistance_diff;

                if cap_diff_ok && res_diff_ok {
                    group_cells.push(cells.remove(i));
                } else {
                    i += 1;
                }
            }

            if group_cells.len() == self.config.cells_per_group {
                groups.push(self.create_battery_group(group_cells, batch_id, group_number));
            } else {
                cells.extend(group_cells.into_iter().skip(1));
            }
        }

        groups
    }

    fn genetic_grouping(&self, cells: Vec<CellInfo>, batch_id: &str) -> Vec<BatteryGroup> {
        let num_groups = cells.len() / self.config.cells_per_group;
        if num_groups == 0 {
            return Vec::new();
        }

        let params = &self.config.genetic_params;
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
        let mut best_individual = None;

        for _generation in 0..params.max_generations {
            let fitnesses: Vec<f64> = population
                .iter()
                .map(|ind| self.fitness_function(ind, &cells, num_groups))
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
                let parent1 = self.tournament_selection(&population, &fitnesses, 3, &mut rng);
                let parent2 = self.tournament_selection(&population, &fitnesses, 3, &mut rng);

                let child = if rng.gen::<f64>() < params.crossover_rate {
                    self.order_crossover(&population[parent1], &population[parent2], &mut rng)
                } else {
                    population[parent1].clone()
                };

                let child = if rng.gen::<f64>() < params.mutation_rate {
                    self.swap_mutation(&child, &mut rng)
                } else {
                    child
                };

                new_population.push(child);
            }

            population = new_population;
        }

        match best_individual {
            Some(ind) => self.decode_individual(&ind, &cells, batch_id, num_groups),
            None => self.greedy_grouping(cells, batch_id),
        }
    }

    fn fitness_function(&self, individual: &[usize], cells: &[CellInfo], num_groups: usize) -> f64 {
        let mut total_score = 0.0;

        for group_idx in 0..num_groups {
            let start = group_idx * self.config.cells_per_group;
            let end = start + self.config.cells_per_group;
            if end > cells.len() {
                break;
            }

            let group_cells: Vec<&CellInfo> = individual[start..end]
                .iter()
                .filter_map(|&idx| cells.get(idx))
                .collect();

            if group_cells.len() < self.config.cells_per_group {
                continue;
            }

            let score = self.calculate_group_consistency(&group_cells);
            total_score += score;
        }

        total_score
    }

    fn calculate_group_consistency(&self, cells: &[&CellInfo]) -> f64 {
        if cells.len() < 2 {
            return 0.0;
        }

        let capacities: Vec<f64> = cells.iter().map(|c| c.measured_capacity).collect();
        let resistances: Vec<f64> = cells.iter().map(|c| c.internal_resistance).collect();

        let cap_mean = capacities.iter().sum::<f64>() / capacities.len() as f64;
        let cap_std = self.calculate_std(&capacities, cap_mean);
        let cap_max_diff = capacities.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - capacities.iter().cloned().fold(f64::INFINITY, f64::min);

        let res_mean = resistances.iter().sum::<f64>() / resistances.len() as f64;
        let res_std = self.calculate_std(&resistances, res_mean);
        let res_max_diff = resistances.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - resistances.iter().cloned().fold(f64::INFINITY, f64::min);

        let cap_cv = cap_std / cap_mean;
        let res_cv = res_std / res_mean;

        let cap_diff_penalty = if cap_max_diff > self.config.max_capacity_diff {
            (cap_max_diff - self.config.max_capacity_diff) * 100.0
        } else {
            0.0
        };

        let res_diff_penalty = if res_max_diff > self.config.max_resistance_diff {
            (res_max_diff - self.config.max_resistance_diff) * 10.0
        } else {
            0.0
        };

        let consistency = 100.0 * (1.0 - cap_cv * 10.0) * (1.0 - res_cv * 10.0)
            - cap_diff_penalty
            - res_diff_penalty;

        consistency.max(0.0)
    }

    fn calculate_std(&self, values: &[f64], mean: f64) -> f64 {
        let variance: f64 = values
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / values.len() as f64;
        variance.sqrt()
    }

    fn tournament_selection(
        &self,
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

    fn order_crossover(&self, parent1: &[usize], parent2: &[usize], rng: &mut impl Rng) -> Vec<usize> {
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

    fn swap_mutation(&self, individual: &[usize], rng: &mut impl Rng) -> Vec<usize> {
        let mut child = individual.to_vec();
        let idx1 = rng.gen_range(0..child.len());
        let idx2 = rng.gen_range(0..child.len());
        child.swap(idx1, idx2);
        child
    }

    fn decode_individual(
        &self,
        individual: &[usize],
        cells: &[CellInfo],
        batch_id: &str,
        num_groups: usize,
    ) -> Vec<BatteryGroup> {
        let mut groups = Vec::new();

        for group_idx in 0..num_groups {
            let start = group_idx * self.config.cells_per_group;
            let end = start + self.config.cells_per_group;
            if end > cells.len() {
                break;
            }

            let group_cells: Vec<CellInfo> = individual[start..end]
                .iter()
                .filter_map(|&idx| cells.get(idx).cloned())
                .collect();

            if group_cells.len() == self.config.cells_per_group {
                groups.push(self.create_battery_group(group_cells, batch_id, (group_idx + 1) as u32));
            }
        }

        groups
    }

    fn create_battery_group(&self, cells: Vec<CellInfo>, batch_id: &str, group_number: u32) -> BatteryGroup {
        let capacities: Vec<f64> = cells.iter().map(|c| c.measured_capacity).collect();
        let resistances: Vec<f64> = cells.iter().map(|c| c.internal_resistance).collect();

        let avg_capacity = capacities.iter().sum::<f64>() / capacities.len() as f64;
        let capacity_std = self.calculate_std(&capacities, avg_capacity);
        let capacity_max_diff = capacities.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - capacities.iter().cloned().fold(f64::INFINITY, f64::min);

        let avg_resistance = resistances.iter().sum::<f64>() / resistances.len() as f64;
        let resistance_std = self.calculate_std(&resistances, avg_resistance);
        let resistance_max_diff = resistances.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - resistances.iter().cloned().fold(f64::INFINITY, f64::min);

        let cell_refs: Vec<&CellInfo> = cells.iter().collect();
        let consistency_score = self.calculate_group_consistency(&cell_refs);

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
            algorithm: self.config.algorithm,
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
