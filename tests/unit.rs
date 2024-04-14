use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use core::task::Waker;
use rand::Rng;
use std::sync::Arc;
use std::thread;
use wakerpool::WakerList;

struct Task {
    wake_count: AtomicU64,
}

impl Task {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            wake_count: AtomicU64::new(0),
        })
    }

    fn waker(self: &Arc<Self>) -> Waker {
        self.clone().into()
    }

    fn wake_count(&self) -> u64 {
        self.wake_count.load(Ordering::Acquire)
    }
}

impl std::task::Wake for Task {
    fn wake(self: Arc<Self>) {
        self.wake_count.fetch_add(1, Ordering::AcqRel);
    }
}

#[test]
fn marker_traits() {
    use static_assertions::assert_impl_all;
    use static_assertions::assert_not_impl_any;
    assert_impl_all!(WakerList: Send, Unpin);
    assert_not_impl_any!(WakerList: Sync);
}

#[test]
fn default_list_is_empty() {
    let wl: WakerList = Default::default();
    assert!(wl.is_empty());
}

#[test]
fn wake_one() {
    let task = Task::new();

    let mut wl = WakerList::new();
    wl.push(task.waker());
    wl.pop().unwrap().wake();

    assert_eq!(1, task.wake_count());
}

#[test]
fn drop_list_with_waker() {
    let task = Task::new();

    let mut wl = WakerList::new();
    wl.push(task.waker());

    // TODO: assert pool size
}

#[test]
fn drop_list_on_another_thread() {
    let task = Task::new();

    let mut wl = WakerList::new();
    wl.push(task.waker());
    thread::spawn(move || {
        drop(wl);
    })
    .join()
    .unwrap();

    let mut wl = WakerList::new();
    wl.push(task.waker());
}

#[test]
fn stress() {
    const I: usize = if cfg!(miri) { 100 } else { 100000 };
    const J: usize = if cfg!(miri) { 10 } else { 100 };
    let thread_count = thread::available_parallelism().unwrap().get();

    let task = Task::new();

    let mut jh = Vec::with_capacity(thread_count);
    for _ in 0..thread_count {
        let task = task.clone();
        jh.push(thread::spawn(move || {
            let mut rng = rand::thread_rng();
            for _ in 0..I {
                let mut wl = WakerList::new();
                for _ in 0..rng.gen_range(0..J) {
                    wl.push(task.waker());
                }
            }
        }));
    }
    drop(task);
    for h in jh {
        h.join().unwrap();
    }
}
