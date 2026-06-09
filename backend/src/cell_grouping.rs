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
    pub time_limit_ms: u64,
    pub enable_parallel: bool,
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
            enable_parallel: true,
            large_dataset_threshold: 1000,
            fallback_to_greedy_on_timeout: true,
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
        let start_time = std::time::Instant::now();

        if cells.len() >= params.large_dataset_threshold {
            return self.genetic_grouping_divide_conquer(cells, batch_id, num_groups);
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

        if timed_out && best_individual.is_none() && params.fallback_to_greedy_on_timeout {
            return self.greedy_grouping(cells, batch_id);
        }

        match best_individual {
            Some(ind) => self.decode_individual(&ind, &cells, batch_id, num_groups),
            None => self.greedy_grouping(cells, batch_id),
        }
    }

    fn genetic_grouping_divide_conquer(
        &self,
        mut cells: Vec<CellInfo>,
        batch_id: &str,
        num_groups: usize,
    ) -> Vec<BatteryGroup> {
        let params = &self.config.genetic_params;
        let start_time = std::time::Instant::now();

        cells.sort_by(|a, b| a.measured_capacity.partial_cmp(&b.measured_capacity).unwrap());

        let chunk_size = (cells.len() / 4).max(params.large_dataset_threshold);
        let mut all_groups = Vec::new();
        let mut chunk_offset = 0;

        for (chunk_idx, chunk) in cells.chunks(chunk_size).enumerate() {
            if start_time.elapsed().as_millis() as u64 >= params.time_limit_ms {
                let remaining: Vec<CellInfo> = cells.chunks(chunk_size).skip(chunk_idx)
                    .flatten().cloned().collect();
                let greedy_groups = self.greedy_grouping_with_offset(
                    remaining, batch_id, all_groups.len() as u32, chunk_offset
                );
                all_groups.extend(greedy_groups);
                break;
            }

            let chunk_cells: Vec<CellInfo> = chunk.to_vec();
            let chunk_num_groups = chunk_cells.len() / self.config.cells_per_group;
            
            if chunk_num_groups > 0 {
                let sub_params = GeneticParams {
                    population_size: params.population_size / 2,
                    max_generations: params.max_generations / 2,
                    time_limit_ms: params.time_limit_ms / 4,
                    ..params.clone()
                };
                
                let sub_config = GroupingConfig {
                    genetic_params: sub_params,
                    ..self.config.clone()
                };
                
                let sub_service = CellGroupingService::new(sub_config);
                let sub_groups = sub_service.genetic_grouping_standard(
                    chunk_cells, batch_id, all_groups.len() as u32, chunk_offset
                );
                
                let grouped_count = sub_groups.len() * self.config.cells_per_group;
                chunk_offset += grouped_count;
                all_groups.extend(sub_groups);
            }
        }

        all_groups
    }

    fn genetic_grouping_standard(
        &self,
        cells: Vec<CellInfo>,
        batch_id: &str,
        group_number_offset: u32,
        cell_id_offset: usize,
    ) -> Vec<BatteryGroup> {
        let num_groups = cells.len() / self.config.cells_per_group;
        if num_groups == 0 {
            return Vec::new();
        }

        let params = &self.config.genetic_params;
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
            Some(ind) => self.decode_individual_with_offset(
                &ind, &cells, batch_id, num_groups, group_number_offset, cell_id_offset
            ),
            None => self.greedy_grouping_with_offset(cells, batch_id, group_number_offset, cell_id_offset),
        }
    }

    fn greedy_grouping_with_offset(
        &self,
        mut cells: Vec<CellInfo>,
        batch_id: &str,
        group_number_offset: u32,
        _cell_id_offset: usize,
    ) -> Vec<BatteryGroup> {
        cells.sort_by(|a, b| b.capacity_ratio.partial_cmp(&a.capacity_ratio).unwrap());

        let mut groups = Vec::new();
        let mut group_number = group_number_offset;

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

    fn decode_individual_with_offset(
        &self,
        individual: &[usize],
        cells: &[CellInfo],
        batch_id: &str,
        num_groups: usize,
        group_number_offset: u32,
        _cell_id_offset: usize,
    ) -> Vec<BatteryGroup> {
        let mut groups = Vec::new();

        for group_idx in 0..num_groups {
            let start = group_idx * self.config.cells_per_group;
            let end = start + self.config.cells_per_group;

            let group_cells: Vec<CellInfo> = (start..end)
                .map(|i| cells[individual[i]].clone())
                .collect();

            groups.push(self.create_battery_group(
                group_cells,
                batch_id,
                group_number_offset + group_idx as u32 + 1,
            ));
        }

        groups
    }

    fn calculate_std(values: &[f64]) -> f64 {
        if values.len() < 2 {
            return 0.0;
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        variance.sqrt()
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

    #[test]
    fn test_genetic_algorithm_minimizes_capacity_diff() {
        let cells = generate_test_cells(100, 0.03, 1.0);

        let config = GroupingConfig {
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
            },
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells.clone(), "TEST-BATCH-001".to_string());

        assert!(result.group_count > 0, "Should create at least one group");

        for group in &result.groups {
            assert!(group.capacity_max_diff < 0.1,
                "Capacity max diff {:.4} should be < 0.1", group.capacity_max_diff);
            assert!(group.capacity_std < 0.03,
                "Capacity std {:.4} should be < 0.03", group.capacity_std);
        }
    }

    #[test]
    fn test_genetic_algorithm_minimizes_resistance_diff() {
        let cells = generate_test_cells(100, 0.02, 2.0);

        let config = GroupingConfig {
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
            },
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells.clone(), "TEST-BATCH-001".to_string());

        for group in &result.groups {
            assert!(group.resistance_max_diff < 5.0,
                "Resistance max diff {:.4} should be < 5.0", group.resistance_max_diff);
            assert!(group.resistance_std < 2.0,
                "Resistance std {:.4} should be < 2.0", group.resistance_std);
        }
    }

    #[test]
    fn test_genetic_vs_greedy_comparison() {
        let cells = generate_test_cells(100, 0.03, 1.5);

        let genetic_config = GroupingConfig {
            algorithm: GroupingAlgorithm::Genetic,
            ..GroupingConfig::default()
        };
        let greedy_config = GroupingConfig {
            algorithm: GroupingAlgorithm::Greedy,
            ..GroupingConfig::default()
        };

        let genetic_service = CellGroupingService::new(genetic_config);
        let greedy_service = CellGroupingService::new(greedy_config);

        let genetic_result = genetic_service.group_cells(cells.clone(), "TEST-BATCH-001".to_string());
        let greedy_result = greedy_service.group_cells(cells.clone(), "TEST-BATCH-001".to_string());

        assert!(genetic_result.avg_consistency_score >= greedy_result.avg_consistency_score * 0.95,
            "Genetic avg consistency {:.2} should be >= greedy {:.2} * 0.95",
            genetic_result.avg_consistency_score, greedy_result.avg_consistency_score);
    }

    #[test]
    fn test_grouping_list_format_correctness() {
        let cells = generate_test_cells(50, 0.02, 1.0);

        let config = GroupingConfig {
            cells_per_group: 10,
            ..GroupingConfig::default()
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells, "TEST-BATCH-001".to_string());

        assert_eq!(result.batch_id, "TEST-BATCH-001");
        assert_eq!(result.cells_per_group, 10);
        assert!(result.group_count <= 5);

        for (i, group) in result.groups.iter().enumerate() {
            assert_eq!(group.group_number, (i + 1) as u32);
            assert_eq!(group.batch_id, "TEST-BATCH-001");
            assert_eq!(group.cell_count, 10);
            assert_eq!(group.cell_ids.len(), 10);
            assert!(!group.group_id.is_empty());
            assert!(group.avg_capacity > 0.0);
            assert!(group.capacity_std >= 0.0);
            assert!(group.consistency_score >= 0.0 && group.consistency_score <= 100.0);
        }
    }

    #[test]
    fn test_consistency_score_calculation_accuracy() {
        let config = GroupingConfig::default();
        let service = CellGroupingService::new(config);

        let cells = vec![
            generate_test_cells(1, 0.0, 0.0).remove(0),
            generate_test_cells(1, 0.0, 0.0).remove(0),
            generate_test_cells(1, 0.0, 0.0).remove(0),
        ];

        let cell_refs: Vec<&CellInfo> = cells.iter().collect();
        let score = service.calculate_group_consistency(&cell_refs);

        assert!(score > 90.0, "Identical cells should have high consistency score, got {:.2}", score);

        let cells_diff = vec![
            generate_test_cells(1, 0.0, 0.0).remove(0),
            generate_test_cells(1, 0.05, 2.0).remove(0),
            generate_test_cells(1, 0.08, 3.0).remove(0),
        ];

        let cell_refs_diff: Vec<&CellInfo> = cells_diff.iter().collect();
        let score_diff = service.calculate_group_consistency(&cell_refs_diff);

        assert!(score_diff < score, "Different cells should have lower consistency score");
    }

    #[test]
    fn test_boundary_insufficient_cells() {
        let cells = generate_test_cells(5, 0.02, 1.0);

        let config = GroupingConfig {
            cells_per_group: 10,
            ..GroupingConfig::default()
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells, "TEST-BATCH-001".to_string());

        assert_eq!(result.group_count, 0);
        assert_eq!(result.groups.len(), 0);
        assert_eq!(result.avg_consistency_score, 0.0);
    }

    #[test]
    fn test_boundary_all_cells_rejected() {
        let mut cells = generate_test_cells(20, 0.02, 1.0);
        for cell in &mut cells {
            cell.capacity_ratio = 0.8;
            cell.grade = CellGrade::Rejected;
        }

        let config = GroupingConfig {
            min_capacity_ratio: 0.85,
            ..GroupingConfig::default()
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells, "TEST-BATCH-001".to_string());

        assert_eq!(result.rejected_cells, 20);
        assert_eq!(result.group_count, 0);
    }

    #[test]
    fn test_boundary_exact_group_count() {
        let cells = generate_test_cells(100, 0.02, 1.0);

        let config = GroupingConfig {
            cells_per_group: 10,
            ..GroupingConfig::default()
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells, "TEST-BATCH-001".to_string());

        assert_eq!(result.group_count, 10);
        assert_eq!(result.total_cells, 100);
    }

    #[test]
    fn test_consistency_score_edge_cases() {
        let config = GroupingConfig::default();
        let service = CellGroupingService::new(config);

        let single_cell = generate_test_cells(1, 0.0, 0.0);
        let score = service.calculate_group_consistency(&[&single_cell[0]]);
        assert_eq!(score, 0.0, "Single cell should have 0 consistency score");

        let empty: Vec<&CellInfo> = Vec::new();
        let score_empty = service.calculate_group_consistency(&empty);
        assert_eq!(score_empty, 0.0, "Empty cells should have 0 consistency score");
    }

    #[test]
    fn test_genetic_algorithm_elitism_preserves_best() {
        let cells = generate_test_cells(50, 0.03, 1.5);

        let config = GroupingConfig {
            cells_per_group: 10,
            algorithm: GroupingAlgorithm::Genetic,
            genetic_params: GeneticParams {
                population_size: 30,
                max_generations: 20,
                mutation_rate: 0.1,
                crossover_rate: 0.8,
                elite_count: 2,
            },
            ..GroupingConfig::default()
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells, "TEST-BATCH-001".to_string());

        assert!(result.avg_consistency_score > 70.0,
            "With elitism, consistency score should be > 70, got {:.2}",
            result.avg_consistency_score);
    }

    #[test]
    fn test_greedy_algorithm_sorts_correctly() {
        let mut cells = generate_test_cells(20, 0.05, 2.0);
        cells[0].measured_capacity = RATED_CAPACITY * 0.99;
        cells[1].measured_capacity = RATED_CAPACITY * 0.85;

        let config = GroupingConfig {
            cells_per_group: 10,
            algorithm: GroupingAlgorithm::Greedy,
            ..GroupingConfig::default()
        };

        let service = CellGroupingService::new(config);
        let result = service.group_cells(cells, "TEST-BATCH-001".to_string());

        assert!(result.groups.len() >= 1);
        if let Some(first_group) = result.groups.first() {
            assert!(first_group.avg_capacity > RATED_CAPACITY * 0.9,
                "First group should have higher capacity cells, got {:.4}", first_group.avg_capacity);
        }
    }
}
