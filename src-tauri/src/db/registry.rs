//! The one place that maps an engine to its driver.

use super::drivers::postgres::Postgres;
use super::{DbDriver, DbEngine, EngineInfo};

pub fn driver(engine: DbEngine) -> &'static dyn DbDriver {
    match engine {
        DbEngine::Postgres => &Postgres,
    }
}

pub fn engines() -> Vec<EngineInfo> {
    DbEngine::ALL.iter().map(|e| driver(*e).info()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_has_a_matching_driver() {
        for e in DbEngine::ALL {
            assert_eq!(driver(*e).info().engine, *e);
        }
    }
}
