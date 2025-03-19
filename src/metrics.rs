use std::{
    cell::RefCell, collections::HashMap, error::Error, rc::{Rc, Weak}
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use regex::Regex;

pub trait Metric {
    fn enable(&self) -> bool;
    fn name(&self) -> &String;
    fn export(&self, dpath: &String) -> Result<(), Box<dyn Error>>;
}
// TODO: either read about casting or actually convert to enum instead of trait
// as rust does nto really support inheritance and I am trying to do
// inheritance here.

pub struct CsvMetric {
    name: String,
    enable: bool,

    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}
// TODO: use Generic: Serialize instead of Vec<String> as a row

impl Metric for CsvMetric {
    fn enable(&self) -> bool {
        self.enable
    }

    fn name(&self) -> &String {
        &self.name
    }

    fn export(&self, dpath: &String) -> Result<(), Box<dyn Error>> {
        if !self.enable || self.rows.is_empty() {
            return Ok(());
        }

        std::fs::create_dir_all(dpath)?;

        let fpath = format!("{}/{}.csv", dpath, self.name);
        let mut wtr = csv::Writer::from_path(fpath).unwrap();
        wtr.write_record(&self.columns)?;
        for row in &self.rows {
            wtr.write_record(row)?;
            // wtr.serialize(row)?;
        }
        wtr.flush()?;

        Ok(())
    }
}

impl CsvMetric {
    fn new(name: String, enable: bool, columns: Vec<String>) -> Self {
        Self {
            name,
            enable,
            columns,
            rows: Vec::new(),
        }
    }

    pub fn log(&mut self, row: Vec<String>) {
        if self.enable {
            self.rows.push(row);
        }
    }

    pub fn get_row_count(&self) -> usize {
        self.rows.len()
    }
}

pub trait CsvMetricStruct: Serialize + Default {
    fn to_row(&self) -> Vec<String> {
        let (_, values) = struct_to_vec(&self);
        values
    }

    fn get_columns() -> Vec<String> {
        let (keys, _) = struct_to_vec(&Self::default());
        keys
    }
}

// From ChatGPT!!
fn struct_to_vec<T: Serialize>(object: &T) -> (Vec<String>, Vec<String>) {
    let value = serde_json::to_value(object).expect("Serialization failed");
    if let Value::Object(map) = value {
        let keys = map.keys().cloned().collect::<Vec<_>>();
        // let values = map.values().map(|v| v.to_string()).collect::<Vec<_>>();
        let values = map
            .values()
            .map(|v| match v {
                Value::Number(num) => {
                    if num.is_f64() {
                        // Format floating-point numbers to 2 decimal places
                        format!("{:.2}", num.as_f64().unwrap())
                    } else {
                        // Keep integers as they are
                        num.to_string()
                    }
                }
                _ => v.to_string(), // Handle other types
            })
            .collect::<Vec<_>>();
        (keys, values)
    } else {
        panic!("Expected a flat struct, got something else!");
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricFilter {
    regex: String,
    enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricConfig {
    data_dir: String,
    filters: Vec<MetricFilter>,
}

impl MetricConfig {
    pub fn new(metric_config_file: &str, data_dir: Option<String>) -> Self {
        let metric_config_reader = std::fs::File::open(metric_config_file).unwrap();
        let mut metric_config: MetricConfig = serde_json::from_reader(metric_config_reader).unwrap();
        if data_dir.is_some() {
            metric_config.data_dir = data_dir.unwrap();
        }
        metric_config
    }
}

pub struct MetricRegistry {
    config: MetricConfig,
    csv_metrics: HashMap<String, Rc<RefCell<CsvMetric>>>,
}
// TODO: currently both MetricRegistry and Metric store a copy of the
// string. Ideally only one place should keep it. We'd want to annotate
// that Metric and String share the same lifetime.

// TODO: ideally we'd want one metric registry to be shared everywhere.
// Since CC uses it we'd need to allow CC to store reference to it and
// track lifetimes of CC objects.

impl MetricRegistry {
    pub fn new(config: MetricConfig) -> Self {
        Self {
            config,
            csv_metrics: HashMap::new(),
        }
    }

    fn is_enabled(&self, name: &str) -> bool {
        for filter in &self.config.filters {
            let r = Regex::new(&filter.regex).unwrap();
            if r.is_match(name) {
                return filter.enabled;
            }
        }
        false
    }

    pub fn register_csv_metric(
        &mut self,
        passed_name: &str,
        columns: Vec<String>,
    ) -> Option<Rc<RefCell<CsvMetric>>> {
        let name: String = passed_name.to_string();
        let enable = self.is_enabled(&name);

        if !self.csv_metrics.contains_key(&name) {
            let csv_metric = Rc::new(RefCell::new(CsvMetric::new(name.clone(), enable, columns)));
            self.csv_metrics.insert(name.clone(), csv_metric);
        }
        let csv_metric = self.csv_metrics.get(&name).unwrap();
        Some(Rc::clone(&csv_metric))
    }

    pub fn finish(&self) {
        for (_, metric) in &self.csv_metrics {
            metric.borrow().export(&self.config.data_dir).unwrap();
        }
    }
}
