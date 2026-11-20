//! Long-lived background services shared by widgets: async runtime, trade
//! client, price index, leagues. Filled in once `rpt-trade` lands.

pub struct Services {
    pub rt: tokio::runtime::Runtime,
}

impl Services {
    pub fn new(_cfg: &crate::config::Config, _ctx: egui::Context) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("rpt-net").build().expect("tokio runtime");
        Self { rt }
    }
}
