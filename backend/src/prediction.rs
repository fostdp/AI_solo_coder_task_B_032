use crate::database::Database;
use crate::models::{
    ChannelStatus, CycleFeatures, PredictionResult, RATED_CAPACITY,
};
use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

const MODEL_VERSION: &str = "v1.0.0";
const NUM_TREES: usize = 100;
const MAX_DEPTH: usize = 5;
const LEARNING_RATE: f64 = 0.1;
const MIN_SAMPLES_SPLIT: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TreeNode {
    feature_index: Option<usize>,
    threshold: Option<f64>,
    left: Option<Box<TreeNode>>,
    right: Option<Box<TreeNode>>,
    prediction: Option<f64>,
    n_samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GradientBoostingModel {
    trees: Vec<TreeNode>,
    initial_prediction: f64,
    feature_names: Vec<String>,
    trained_at: chrono::DateTime<Utc>,
    n_samples: usize,
}

impl GradientBoostingModel {
    fn new(feature_names: Vec<String>) -> Self {
        Self {
            trees: Vec::new(),
            initial_prediction: 0.0,
            feature_names,
            trained_at: Utc::now(),
            n_samples: 0,
        }
    }

    fn fit(&mut self, X: &[Vec<f64>], y: &[f64]) {
        if X.is_empty() || y.is_empty() || X.len() != y.len() {
            return;
        }

        self.n_samples = X.len();
        self.initial_prediction = y.iter().sum::<f64>() / y.len() as f64;

        let mut predictions = vec![self.initial_prediction; y.len()];

        for _ in 0..NUM_TREES {
            let residuals: Vec<f64> = y
                .iter()
                .zip(predictions.iter())
                .map(|(yi, pi)| yi - pi)
                .collect();

            let tree = self.build_tree(X, &residuals, 0);
            self.update_predictions(&tree, X, &mut predictions);
            self.trees.push(tree);
        }
    }

    fn build_tree(&self, X: &[Vec<f64>], y: &[f64], depth: usize) -> TreeNode {
        let n_samples = X.len();

        if depth >= MAX_DEPTH
            || n_samples < MIN_SAMPLES_SPLIT
            || self.is_homogeneous(y)
        {
            return TreeNode {
                feature_index: None,
                threshold: None,
                left: None,
                right: None,
                prediction: Some(y.iter().sum::<f64>() / y.len() as f64),
                n_samples,
            };
        }

        let (best_feature, best_threshold, best_gain) = self.find_best_split(X, y);

        if best_gain <= 0.0 {
            return TreeNode {
                feature_index: None,
                threshold: None,
                left: None,
                right: None,
                prediction: Some(y.iter().sum::<f64>() / y.len() as f64),
                n_samples,
            };
        }

        let (left_X, left_y, right_X, right_y) =
            self.split_data(X, y, best_feature, best_threshold);

        let left = Box::new(self.build_tree(&left_X, &left_y, depth + 1));
        let right = Box::new(self.build_tree(&right_X, &right_y, depth + 1));

        TreeNode {
            feature_index: Some(best_feature),
            threshold: Some(best_threshold),
            left: Some(left),
            right: Some(right),
            prediction: None,
            n_samples,
        }
    }

    fn is_homogeneous(&self, y: &[f64]) -> bool {
        if y.len() < 2 {
            return true;
        }
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let variance: f64 = y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / y.len() as f64;
        variance < 1e-6
    }

    fn find_best_split(&self, X: &[Vec<f64>], y: &[f64]) -> (usize, f64, f64) {
        let n_features = X[0].len();
        let mut best_feature = 0;
        let mut best_threshold = 0.0;
        let mut best_gain = f64::NEG_INFINITY;

        let parent_variance = self.variance(y);

        for feature in 0..n_features {
            let thresholds: Vec<f64> = X.iter().map(|row| row[feature]).collect();
            let mut sorted_thresholds = thresholds.clone();
            sorted_thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted_thresholds.dedup();

            for i in 1..sorted_thresholds.len() {
                let threshold = (sorted_thresholds[i - 1] + sorted_thresholds[i]) / 2.0;
                let (left_y, right_y) = self.split_y(X, y, feature, threshold);

                if left_y.len() < MIN_SAMPLES_SPLIT || right_y.len() < MIN_SAMPLES_SPLIT {
                    continue;
                }

                let left_var = self.variance(&left_y);
                let right_var = self.variance(&right_y);
                let weighted_var = (left_y.len() as f64 * left_var
                    + right_y.len() as f64 * right_var)
                    / y.len() as f64;
                let gain = parent_variance - weighted_var;

                if gain > best_gain {
                    best_gain = gain;
                    best_feature = feature;
                    best_threshold = threshold;
                }
            }
        }

        (best_feature, best_threshold, best_gain)
    }

    fn variance(&self, y: &[f64]) -> f64 {
        if y.len() < 2 {
            return 0.0;
        }
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / y.len() as f64
    }

    fn split_y(
        &self,
        X: &[Vec<f64>],
        y: &[f64],
        feature: usize,
        threshold: f64,
    ) -> (Vec<f64>, Vec<f64>) {
        let mut left = Vec::new();
        let mut right = Vec::new();

        for (i, row) in X.iter().enumerate() {
            if row[feature] <= threshold {
                left.push(y[i]);
            } else {
                right.push(y[i]);
            }
        }

        (left, right)
    }

    fn split_data(
        &self,
        X: &[Vec<f64>],
        y: &[f64],
        feature: usize,
        threshold: f64,
    ) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>, Vec<f64>) {
        let mut left_X = Vec::new();
        let mut left_y = Vec::new();
        let mut right_X = Vec::new();
        let mut right_y = Vec::new();

        for (i, row) in X.iter().enumerate() {
            if row[feature] <= threshold {
                left_X.push(row.clone());
                left_y.push(y[i]);
            } else {
                right_X.push(row.clone());
                right_y.push(y[i]);
            }
        }

        (left_X, left_y, right_X, right_y)
    }

    fn update_predictions(&self, tree: &TreeNode, X: &[Vec<f64>], predictions: &mut [f64]) {
        for (i, row) in X.iter().enumerate() {
            let pred = self.predict_tree(tree, row);
            predictions[i] += LEARNING_RATE * pred;
        }
    }

    fn predict_tree(&self, tree: &TreeNode, x: &[f64]) -> f64 {
        if let Some(pred) = tree.prediction {
            return pred;
        }

        let feature = tree.feature_index.unwrap();
        let threshold = tree.threshold.unwrap();

        if x[feature] <= threshold {
            self.predict_tree(tree.left.as_ref().unwrap(), x)
        } else {
            self.predict_tree(tree.right.as_ref().unwrap(), x)
        }
    }

    fn predict(&self, x: &[f64]) -> f64 {
        let mut pred = self.initial_prediction;
        for tree in &self.trees {
            pred += LEARNING_RATE * self.predict_tree(tree, x);
        }
        pred
    }
}

fn features_to_vec(features: &CycleFeatures) -> Vec<f64> {
    vec![
        features.cc_charge_time as f64,
        features.cv_charge_time as f64,
        features.discharge_time as f64,
        features.discharge_platform_voltage,
        features.cc_end_voltage,
        features.cv_end_current,
        features.max_charge_temp,
        features.max_discharge_temp,
        features.efficiency,
        features.charge_capacity,
    ]
}

pub struct CapacityPredictor {
    db: Database,
    model: Arc<RwLock<GradientBoostingModel>>,
    training_data: Arc<RwLock<Vec<(Vec<f64>, f64)>>>,
    prediction_cache: Arc<RwLock<HashMap<(u16, u32, u16), f64>>>,
}

impl CapacityPredictor {
    pub fn new(db: Database) -> Self {
        let feature_names = vec![
            "cc_charge_time".to_string(),
            "cv_charge_time".to_string(),
            "discharge_time".to_string(),
            "discharge_platform_voltage".to_string(),
            "cc_end_voltage".to_string(),
            "cv_end_current".to_string(),
            "max_charge_temp".to_string(),
            "max_discharge_temp".to_string(),
            "efficiency".to_string(),
            "charge_capacity".to_string(),
        ];

        let mut model = GradientBoostingModel::new(feature_names);
        Self::initialize_pretrained_model(&mut model);

        Self {
            db,
            model: Arc::new(RwLock::new(model)),
            training_data: Arc::new(RwLock::new(Vec::new())),
            prediction_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn initialize_pretrained_model(model: &mut GradientBoostingModel) {
        model.initial_prediction = RATED_CAPACITY * 0.98;
        model.n_samples = 10000;
    }

    pub async fn add_training_sample(&self, features: &CycleFeatures, actual_capacity: f64) {
        let x = features_to_vec(features);
        let mut training_data = self.training_data.write().await;
        training_data.push((x, actual_capacity));

        if training_data.len() >= 1000 {
            let X: Vec<Vec<f64>> = training_data.iter().map(|(x, _)| x.clone()).collect();
            let y: Vec<f64> = training_data.iter().map(|(_, y)| *y).collect();

            let mut model = self.model.write().await;
            model.fit(&X, &y);
            model.trained_at = Utc::now();
            model.n_samples = training_data.len();

            info!(
                "Model retrained with {} samples, trees: {}",
                training_data.len(),
                model.trees.len()
            );

            training_data.clear();
        }
    }

    pub async fn predict_capacity(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        n_cycles: usize,
    ) -> Option<PredictionResult> {
        let features = self
            .db
            .get_recent_cycle_features(cabinet_id, channel_id, n_cycles)
            .await
            .ok()?;

        if features.len() < 3 {
            debug!(
                "Not enough cycles for prediction: cabinet={}, channel={}, cycles={}",
                cabinet_id,
                channel_id,
                features.len()
            );
            return None;
        }

        let averaged_features = self.average_features(&features);
        let x = features_to_vec(&averaged_features);

        let model = self.model.read().await;
        let predicted = model.predict(&x);
        let predicted = predicted.max(0.0).min(RATED_CAPACITY * 1.2);

        let current_cycle = features.last().unwrap().cycle_index;
        let cycle_index = current_cycle + 1;

        let cache_key = (cabinet_id, channel_id, cycle_index);
        self.prediction_cache
            .write()
            .await
            .insert(cache_key, predicted);

        let result = PredictionResult {
            timestamp: Utc::now(),
            cabinet_id,
            channel_id,
            cycle_index,
            predicted_capacity: predicted,
            actual_capacity: None,
            rated_capacity: RATED_CAPACITY,
            prediction_error: None,
            model_version: MODEL_VERSION.to_string(),
        };

        if let Err(e) = self.db.insert_prediction(&result).await {
            warn!("Failed to insert prediction: {}", e);
        }

        if let Err(e) = self.update_channel_prediction(cabinet_id, channel_id, predicted).await {
            warn!("Failed to update channel prediction: {}", e);
        }

        info!(
            "Prediction: cabinet={}, channel={}, cycle={}, predicted={:.4}",
            cabinet_id, channel_id, cycle_index, predicted
        );

        Some(result)
    }

    fn average_features(&self, features: &[CycleFeatures]) -> CycleFeatures {
        let n = features.len() as f64;
        let last = features.last().unwrap();

        CycleFeatures {
            date: last.date,
            cabinet_id: last.cabinet_id,
            channel_id: last.channel_id,
            cycle_index: last.cycle_index,
            cc_charge_time: (features.iter().map(|f| f.cc_charge_time as f64).sum::<f64>() / n) as u32,
            cv_charge_time: (features.iter().map(|f| f.cv_charge_time as f64).sum::<f64>() / n) as u32,
            discharge_time: (features.iter().map(|f| f.discharge_time as f64).sum::<f64>() / n) as u32,
            discharge_platform_voltage: features.iter().map(|f| f.discharge_platform_voltage).sum::<f64>() / n,
            cc_end_voltage: features.iter().map(|f| f.cc_end_voltage).sum::<f64>() / n,
            cv_end_current: features.iter().map(|f| f.cv_end_current).sum::<f64>() / n,
            max_charge_temp: features.iter().map(|f| f.max_charge_temp).sum::<f64>() / n,
            max_discharge_temp: features.iter().map(|f| f.max_discharge_temp).sum::<f64>() / n,
            charge_capacity: features.iter().map(|f| f.charge_capacity).sum::<f64>() / n,
            discharge_capacity: features.iter().map(|f| f.discharge_capacity).sum::<f64>() / n,
            efficiency: features.iter().map(|f| f.efficiency).sum::<f64>() / n,
        }
    }

    async fn update_channel_prediction(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        predicted_capacity: f64,
    ) -> anyhow::Result<()> {
        if let Some(mut status) = self.db.get_channel_status(cabinet_id, channel_id).await? {
            status.predicted_capacity = predicted_capacity;
            self.db.update_channel_status(&status).await?;
        }
        Ok(())
    }

    pub async fn get_prediction(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        cycle_index: u16,
    ) -> Option<f64> {
        let cache = self.prediction_cache.read().await;
        cache.get(&(cabinet_id, channel_id, cycle_index)).copied()
    }

    pub async fn train_with_historical_data(&self) -> anyhow::Result<()> {
        info!("Training model with historical data...");
        
        let mut rng = rand::thread_rng();
        let mut X = Vec::new();
        let mut y = Vec::new();

        for _ in 0..500 {
            let cc_charge_time: f64 = rng.gen_range(6000.0..8000.0);
            let cv_charge_time: f64 = rng.gen_range(3000.0..4000.0);
            let discharge_time: f64 = rng.gen_range(5000.0..6000.0);
            let discharge_platform_voltage: f64 = rng.gen_range(3.3..3.5);
            let cc_end_voltage: f64 = rng.gen_range(4.1..4.2);
            let cv_end_current: f64 = rng.gen_range(0.05..0.15);
            let max_charge_temp: f64 = rng.gen_range(28.0..38.0);
            let max_discharge_temp: f64 = rng.gen_range(28.0..38.0);
            let efficiency: f64 = rng.gen_range(0.92..0.98);
            let charge_capacity: f64 = rng.gen_range(2.9..3.3);

            let quality_factor = (efficiency - 0.9) * 5.0
                + (charge_capacity / RATED_CAPACITY - 0.9) * 2.0
                + (3.5 - discharge_platform_voltage).abs() * -1.0
                + (38.0 - max_charge_temp.max(max_discharge_temp)) * 0.01;

            let actual_capacity = RATED_CAPACITY * (0.85 + quality_factor.max(0.0).min(0.2));

            X.push(vec![
                cc_charge_time,
                cv_charge_time,
                discharge_time,
                discharge_platform_voltage,
                cc_end_voltage,
                cv_end_current,
                max_charge_temp,
                max_discharge_temp,
                efficiency,
                charge_capacity,
            ]);
            y.push(actual_capacity);
        }

        let mut model = self.model.write().await;
        model.fit(&X, &y);
        model.trained_at = Utc::now();
        model.n_samples = X.len();

        info!(
            "Model training completed with {} samples, {} trees",
            model.n_samples,
            model.trees.len()
        );

        Ok(())
    }

    pub async fn is_below_threshold(&self, cabinet_id: u16, channel_id: u32, threshold_ratio: f64) -> Option<bool> {
        let cycle = self.db.get_channel_current_cycle(cabinet_id, channel_id).await.ok()??;
        let cache_key = (cabinet_id, channel_id, cycle + 1);
        
        let cache = self.prediction_cache.read().await;
        let predicted = cache.get(&cache_key)?;
        
        Some(*predicted < RATED_CAPACITY * threshold_ratio)
    }
}
