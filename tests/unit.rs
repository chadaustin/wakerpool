use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use core::task::Waker;
use std::sync::Arc;
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

/*
#[test]
fn wake_one() {
    let wl = WakerList::new();

}
*/
