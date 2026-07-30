use std::time::Duration;

use backon::Sleeper;
use smol::Timer;

#[derive(Clone, Copy, Debug)]
pub struct SmolSleeper;

impl Sleeper for SmolSleeper {
    type Sleep = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;
    fn sleep(&self, dur: Duration) -> Self::Sleep {
        Box::pin(async move {
            Timer::after(dur).await;
        })
    }
}
