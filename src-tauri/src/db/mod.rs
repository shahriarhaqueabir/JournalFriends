pub mod ai;
pub mod analysis_queue;
pub mod audit;
pub mod events;
pub mod init;
pub mod journal;
pub mod knowledge;
pub mod migrations;
pub mod mood;
pub mod paths;
pub mod pool;
pub mod reports;
pub mod settings;
pub mod tasks;
pub mod trash;
pub mod vault_migrate;
pub mod vectors;

#[cfg(test)]
mod mock;
