use crate::models::Balloon;

pub type BalloonOpt = Box<dyn Fn(&mut Balloon) + Send + Sync + 'static>;

pub fn with_stats_polling_intervals(stats_polling_intervals: i64) -> BalloonOpt {
    Box::new(move |balloon| {
        balloon.stats_polling_intervals = stats_polling_intervals;
    })
}

#[derive(Debug, Clone, Default)]
pub struct BalloonDevice {
    balloon: Balloon,
}

impl BalloonDevice {
    pub fn new(
        amount_mib: i64,
        deflate_on_oom: bool,
        opts: impl IntoIterator<Item = BalloonOpt>,
    ) -> Self {
        let mut balloon = Balloon {
            amount_mib: Some(amount_mib),
            deflate_on_oom: Some(deflate_on_oom),
            ..Balloon::default()
        };

        for opt in opts {
            opt(&mut balloon);
        }

        Self { balloon }
    }

    pub fn build(&self) -> Balloon {
        self.balloon.clone()
    }

    pub fn update_amount_mib(mut self, amount_mib: i64) -> Self {
        self.balloon.amount_mib = Some(amount_mib);
        self
    }

    pub fn update_stats_polling_intervals(mut self, stats_polling_intervals: i64) -> Self {
        self.balloon.stats_polling_intervals = stats_polling_intervals;
        self
    }
}
