use spinlock::Spinlock;

use std::io::Write;
use std::os::unix::io::FromRawFd;

mod spinlock;

fn print(spinlock: std::sync::Arc<Spinlock<std::fs::File>>) {
    loop {
        spinlock.lock()
            .expect("poisoned")
            .write_fmt(format_args!("{:?}\n", std::thread::current().id()))
            .expect("couldn't write to stdout");
    }
}

fn main() {
    static NUM_THREADS: usize = 8;

    let stdout = unsafe { std::fs::File::from_raw_fd(1) };
    let lock = std::sync::Arc::new(Spinlock::new(stdout));

    let threads = (0..NUM_THREADS).into_iter().map(|_| {
        let cloned = lock.clone();

        std::thread::spawn(move || print(cloned))
    });

    println!("spawned {} threads", NUM_THREADS);

    for thread in threads {
        thread.join().expect("couldn't join thread");
    }
}
