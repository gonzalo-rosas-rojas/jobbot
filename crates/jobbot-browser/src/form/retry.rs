//! The one retry a call gets when a busy browser fails it.

use std::time::Duration;

/// Runs `attempt`, and once more after `pause_ms` if it fails; the second error is the one kept.
pub(super) async fn retry_once<T, E, F, Fut>(pause_ms: u64, mut attempt: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    match attempt().await {
        Ok(value) => Ok(value),
        Err(_) => {
            tokio::time::sleep(Duration::from_millis(pause_ms)).await;
            attempt().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An attempt that fails its first `failures` calls, counting every call.
    fn flaky(
        failures: usize,
        calls: &std::sync::atomic::AtomicUsize,
    ) -> impl FnMut() -> std::future::Ready<Result<usize, String>> + '_ {
        move || {
            let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            std::future::ready(if call <= failures {
                Err(format!("failure {call}"))
            } else {
                Ok(call)
            })
        }
    }

    #[tokio::test]
    async fn a_failed_attempt_is_tried_once_more() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert_eq!(retry_once(0, flaky(1, &calls)).await, Ok(2));
        assert_eq!(calls.into_inner(), 2);
    }

    #[tokio::test]
    async fn a_second_failure_is_returned_and_there_is_no_third_try() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert_eq!(
            retry_once(0, flaky(5, &calls)).await,
            Err("failure 2".to_string())
        );
        assert_eq!(calls.into_inner(), 2);
    }

    #[tokio::test]
    async fn a_first_success_is_not_repeated() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert_eq!(retry_once(0, flaky(0, &calls)).await, Ok(1));
        assert_eq!(calls.into_inner(), 1);
    }
}
