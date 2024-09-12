use std::{
    cell::RefCell, collections::HashMap, error::Error, rc::{Rc, Weak}
};

use serde::{Deserialize, Serialize};

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
        if self.enable == false || self.rows.len() == 0 {
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
    filters: Vec<MetricFilter>,
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
    pub fn new(metric_config_file: &str) -> Self {
        let metric_config_reader = std::fs::File::open(metric_config_file).unwrap();
        Self {
            config: serde_json::from_reader(metric_config_reader).unwrap(),
            csv_metrics: HashMap::new(),
        }
    }

    pub fn register_csv_metric(
        &mut self,
        passed_name: &str,
        columns: Vec<String>,
    ) -> Option<Rc<RefCell<CsvMetric>>> {
        // TODO: check if it should be enabled based on config
        let name: String = passed_name.to_string();
        if !self.csv_metrics.contains_key(&name) {
            let csv_metric = Rc::new(RefCell::new(CsvMetric::new(name.clone(), true, columns)));
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
