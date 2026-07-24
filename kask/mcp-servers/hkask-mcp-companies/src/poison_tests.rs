//! Regression tests for mutex poison recovery patterns.
//!
//! Verifies that `lock().unwrap_or_else(|e| e.into_inner())` correctly
//! recovers from a poisoned mutex, preventing permanent lock poisoning
//! from crashing the MCP server after a single panicking request.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// Simulate mutex poisoning: a thread panics while holding the lock,
    /// then a recovery thread acquires the lock using the poison-safe pattern.
    #[test]
    fn recover_from_poisoned_mutex() {
        let data = Arc::new(Mutex::new(vec![1, 2, 3]));
        let data_clone = Arc::clone(&data);

        let handle = thread::spawn(move || {
            let _guard = data_clone.lock().unwrap_or_else(|e| e.into_inner());
            panic!("simulated crash while holding lock");
        });
        let _ = handle.join();

        let recovered = data.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(*recovered, vec![1, 2, 3]);
    }

    /// Verify the pattern preserves mutable access after poison recovery.
    #[test]
    fn recover_and_mutate_poisoned_mutex() {
        let data = Arc::new(Mutex::new(String::from("hello")));
        let data_clone = Arc::clone(&data);

        let handle = thread::spawn(move || {
            let _guard = data_clone.lock().unwrap_or_else(|e| e.into_inner());
            panic!("simulated crash");
        });
        let _ = handle.join();

        let mut recovered = data.lock().unwrap_or_else(|e| e.into_inner());
        recovered.push_str(" world");
        assert_eq!(*recovered, "hello world");
    }
}
