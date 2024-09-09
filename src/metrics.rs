use std::{collections::HashMap, error::Error, rc::Rc};

use serde::{Deserialize, Serialize};


pub trait Metric {
    fn enable(&self) -> bool;
    fn name(&self) -> &String;
    fn export(&self, dpath: &String) -> Result<(), Box<dyn Error>>;
}

pub struct CsvMetric<T: Serialize> {
    name: String,
    enable: bool,

    columns: Vec<String>,
    rows: Vec<T>,
}

impl<T: Serialize> Metric for CsvMetric<T> {
    fn enable(&self) -> bool {
        self.enable
    }

    fn name(&self) -> &String {
        &self.name
    }

    fn export(&self, dpath: &String) -> Result<(), Box<dyn Error>> {
        if self.enable == false || self.rows.len() == 0 {
            return Ok(());
        }

        let fpath = format!("{}/{}.csv", dpath, self.name);
        let mut wtr = csv::Writer::from_path(fpath).unwrap();
        for row in &self.rows {
            wtr.serialize(row)?;
        }
        wtr.flush()?;
        Ok(())
    }
}

impl<T: Serialize> CsvMetric<T> {
    fn new(name: String, enable: bool, columns: Vec<String>) -> Self {
        Self {
            name,
            enable,
            columns,
            rows: Vec::new(),
        }
    }

    pub fn log(&mut self, row: T) {
        self.rows.push(row);
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
    filter: Vec<MetricFilter>,
}

pub struct MetricRegistry {
    config: MetricConfig,
    metrics: HashMap<String, Rc<dyn Metric>>,
    // TODO: currently both MetricRegistry and Metric store a copy of the
    // string. Ideally only one place should keep it. We'd want to annotate
    // that Metric and String share the same lifetime.
}

impl MetricRegistry {
    // TODO: ideally we'd want one metric registry to be shared everywhere.
    // Since CC uses it we'd need to allow CC to store reference to it and
    // track lifetimes of CC objects.
    pub fn new(metric_config_file: &str) -> Self {
        Self {
            config: serde_json::from_str(metric_config_file).unwrap(),
            metrics: HashMap::new(),
        }
    }

    pub fn register_csv_metric<T: Serialize + 'static>(&mut self, passed_name: &str, columns: Vec<String>) -> Weak<dyn Metric> {
        // TODO: check if it should be enabled based on config
        let name: String = passed_name.to_string();
        if self.metrics.contains_key(&name) {
            return Rc::downgrade(self.metrics.get(&name).unwrap());
        }
        else {
            let metric: Rc<dyn Metric> = Rc::new(CsvMetric::<T>::new(name.clone(), true, columns));
            self.metrics.insert(name, Rc::clone(&metric));
            Rc::downgrade(&metric)
        }
    }

    pub fn finish(&self) {
        for (_, metric) in &self.metrics {
            metric.export(&self.config.data_dir).unwrap();
        }
    }
}