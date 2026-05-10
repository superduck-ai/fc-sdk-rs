use std::future::Future;

pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime for test helper")
        .block_on(future)
}

pub trait BlockingFutureExt: Future + Sized {
    fn block_on(self) -> Self::Output {
        block_on(self)
    }
}

impl<F> BlockingFutureExt for F where F: Future + Sized {}

pub trait AsyncResultExt<T, E>: Future<Output = std::result::Result<T, E>> + Sized {
    fn unwrap(self) -> T
    where
        E: std::fmt::Debug,
    {
        self.block_on().unwrap()
    }

    fn unwrap_err(self) -> E
    where
        T: std::fmt::Debug,
    {
        self.block_on().unwrap_err()
    }

    fn is_ok(self) -> bool {
        self.block_on().is_ok()
    }

    fn is_err(self) -> bool {
        self.block_on().is_err()
    }

    fn expect(self, message: &str) -> T
    where
        E: std::fmt::Debug,
    {
        self.block_on().expect(message)
    }
}

impl<F, T, E> AsyncResultExt<T, E> for F where F: Future<Output = std::result::Result<T, E>> + Sized {}
