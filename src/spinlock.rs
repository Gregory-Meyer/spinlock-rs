extern crate std;

pub struct Spinlock<T: ?Sized> {
    is_locked: std::sync::atomic::AtomicBool,
    is_poisoned: std::sync::atomic::AtomicBool,
    data: std::cell::UnsafeCell<T>,
}

impl<T> Spinlock<T> {
    pub fn new(t: T) -> Spinlock<T> {
        Spinlock {
            is_locked: std::sync::atomic::AtomicBool::new(false),
            is_poisoned: std::sync::atomic::AtomicBool::new(false),
            data: std::cell::UnsafeCell::new(t),
        }
    }
}

impl <T: ?Sized> Spinlock<T> {
    pub fn lock(&self) -> std::sync::LockResult<SpinlockGuard<T>> {
        unsafe { self.raw_lock(); }

        let to_return = SpinlockGuard{ spinlock: self };

        if self.is_poisoned() {
            return Err(std::sync::PoisonError::new(to_return));
        }

        Ok(to_return)
    }

    pub fn try_lock(&self) -> std::sync::TryLockResult<SpinlockGuard<T>> {
        if unsafe { !self.raw_try_lock() } {
            return Err(std::sync::TryLockError::WouldBlock);
        }

        let to_return = SpinlockGuard{ spinlock: self };

        if self.is_poisoned() {
            let error = std::sync::PoisonError::new(to_return);

            return Err(std::sync::TryLockError::Poisoned(error));
        }

        Ok(to_return)
    }

    pub fn is_poisoned(&self) -> bool {
        self.is_poisoned.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn into_inner(self) -> std::sync::LockResult<T> where T: Sized {
        unsafe {
            let (_, poison, data) = {
                let Spinlock {
                    ref is_locked,
                    ref is_poisoned,
                    ref data,
                } = self;

                (
                    std::ptr::read(is_locked),
                    std::ptr::read(is_poisoned),
                    std::ptr::read(data),
                )
            };

            std::mem::forget(self);

            let inner = data.into_inner();

            if poison.load(std::sync::atomic::Ordering::SeqCst) {
                Err(std::sync::PoisonError::new(inner))
            } else {
                Ok(inner)
            }
        }
    }

    pub fn get_mut(&mut self) -> std::sync::LockResult<&mut T> {
        let data = unsafe { &mut *self.data.get() };

        if self.is_poisoned() {
            Err(std::sync::PoisonError::new(data))
        } else {
            Ok(data)
        }
    }

    unsafe fn raw_lock(&self) {
        while !self.raw_try_lock() { }
    }

    unsafe fn raw_try_lock(&self) -> bool {
        !self.is_locked.test_and_set(std::sync::atomic::Ordering::SeqCst)
    }

    unsafe fn raw_unlock(&self) {
        self.is_locked.clear(std::sync::atomic::Ordering::SeqCst);
    }
}

impl<T: ?Sized> std::panic::UnwindSafe for Spinlock<T> { }

impl<T: ?Sized> std::panic::RefUnwindSafe for Spinlock<T> { }

unsafe impl<T: ?Sized + Send> Send for Spinlock<T> { }

unsafe impl<T: ?Sized + Send> Sync for Spinlock<T> { }

impl<T> From<T> for Spinlock<T> {
    fn from(t: T) -> Spinlock<T> {
        Spinlock::new(t)
    }
}

impl<T: ?Sized + Default> Default for Spinlock<T> {
    fn default() -> Spinlock<T> {
        Spinlock::new(T::default())
    }
}

impl<T: ?Sized + std::fmt::Debug> std::fmt::Debug for Spinlock<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.try_lock() {
            Ok(guard) => f.debug_struct("Spinlock")
                .field("data", &&*guard)
                .finish(),
            Err(std::sync::TryLockError::Poisoned(err)) => {
                f.debug_struct("Spinlock")
                    .field("data", &&**err.get_ref())
                    .finish()
            },
            Err(std::sync::TryLockError::WouldBlock) => {
                struct LockedPlaceholder;

                impl std::fmt::Debug for LockedPlaceholder {
                    fn fmt(&self,
                           f: &mut std::fmt::Formatter) -> std::fmt::Result {
                        f.write_str("<locked>")
                    }
                }

                f.debug_struct("Spinlock")
                    .field("data", &LockedPlaceholder)
                    .finish()
            }
        }
    }
}

pub struct SpinlockGuard<'a, T: ?Sized + 'a> {
    spinlock: &'a Spinlock<T>,
}

// impl<'a, T: ?Sized> !Send for SpinlockGuard<'a, T> { }

unsafe impl<'a, T: ?Sized + Sync> Sync for SpinlockGuard<'a, T> { }

impl<'a, T: ?Sized> std::ops::Deref for SpinlockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        match unsafe { self.spinlock.data.get().as_ref() } {
            Some(v) => v,
            None => panic!("data ptr is null"),
        }
    }
}

impl<'a, T: ?Sized> std::ops::DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        match unsafe { self.spinlock.data.get().as_mut() } {
            Some(v) => v,
            None => panic!("data ptr is null"),
        }
    }
}

impl<'a, T: ?Sized + std::fmt::Debug> std::fmt::Debug for SpinlockGuard<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("SpinlockGuard")
            .field("spinlock", &self.spinlock)
            .finish()
    }
}

impl<'a, T: ?Sized + std::fmt::Display> std::fmt::Display for SpinlockGuard<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl<'a, T: ?Sized> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.spinlock.is_poisoned.store(
                true,
                std::sync::atomic::Ordering::SeqCst
            );
        }

        unsafe { self.spinlock.raw_unlock(); }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use spinlock::Spinlock;

    #[test]
    fn already_locked() {
        let spinlock = Spinlock::new(());
        assert!(!spinlock.is_poisoned());

        let guard = spinlock.lock();
        assert!(guard.is_ok());

        let new_guard = spinlock.try_lock();

        match new_guard {
            Err(std::sync::TryLockError::WouldBlock) => assert!(true),
            _ => assert!(false),
        }

        assert!(!spinlock.is_poisoned());
    }

    #[test]
    fn poisoned() {
        let spinlock = Spinlock::new(());
        assert!(!spinlock.is_poisoned());

        let result = std::panic::catch_unwind(|| {
            match spinlock.lock() {
                Ok(_) => {
                    panic!();
                }
                _ => (),
            }
        });

        assert!(result.is_err());
        assert!(spinlock.is_poisoned());
    }
}

trait AtomicFlag {
    fn clear(&self, order: std::sync::atomic::Ordering);

    fn test_and_set(&self, order: std::sync::atomic::Ordering) -> bool;
}

impl AtomicFlag for std::sync::atomic::AtomicBool {
    fn clear(&self, order: std::sync::atomic::Ordering) {
        self.store(false, order);
    }

    fn test_and_set(&self, order: std::sync::atomic::Ordering) -> bool {
        self.swap(true, order)
    }
}
