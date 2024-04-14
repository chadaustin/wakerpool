use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering;
use core::task::Waker;

type WakerNodePtr = AtomicPtr<WakerNode>;

struct WakerNode {
    next: *mut WakerNode,
    waker: MaybeUninit<Waker>,
}

fn allocate_node() -> *mut WakerNode {
    Box::into_raw(Box::new(WakerNode {
        next: ptr::null_mut(),
        waker: MaybeUninit::uninit(),
    }))
}

static GLOBAL_POOL: WakerNodePtr = WakerNodePtr::new(ptr::null_mut());

struct LocalPool {
    head: Cell<*mut WakerNode>,
}

impl LocalPool {
    const fn new() -> LocalPool {
        LocalPool {
            head: Cell::new(ptr::null_mut()),
        }
    }

    fn acquire_node(&self) -> *mut WakerNode {
        let node = self.head.get();
        if !node.is_null() {
            self.head.set(unsafe { (*node).next });
            // We could clear the next pointer, but the caller is
            // responsible.
            return node;
        }

        let mut node = GLOBAL_POOL.load(Ordering::Acquire);
        loop {
            if node.is_null() {
                break;
            }
            // No ABA on global pool because we never deallocate.
            let new_head = unsafe { (*node).next };
            node = match GLOBAL_POOL.compare_exchange_weak(
                node,
                new_head,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(popped) => {
                    return popped;
                }
                Err(node) => node,
            };
        }

        allocate_node()
    }

    unsafe fn release_node(&self, node: *mut WakerNode) {
        unsafe {
            (*node).next = self.head.get();
            self.head.set(node);
        }
    }

    unsafe fn release_list(&self, head: *mut WakerNode) {
        let mut p = self.head.get();
        if p.is_null() {
            self.head.set(head);
            return;
        }
        loop {
            let next = unsafe { (*p).next };
            if next.is_null() {
                break;
            }
            p = next;
        }
        unsafe { (*p).next = head }
    }
}

impl Drop for LocalPool {
    fn drop(&mut self) {
        let mut p = self.head.get();
        if p.is_null() {
            return;
        }
        // Find the tail.
        loop {
            let next = unsafe { (*p).next };
            if next.is_null() {
                break;
            }
            p = next;
        }

        let mut global_head = GLOBAL_POOL.load(Ordering::Acquire);
        loop {
            unsafe {
                (*p).next = global_head;
            }
            global_head = match GLOBAL_POOL.compare_exchange_weak(
                global_head,
                self.head.get(),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(node) => node,
            };
        }
    }
}

thread_local! {
    static LOCAL_POOL: LocalPool = const { LocalPool::new() }
}

fn acquire_node() -> *mut WakerNode {
    LOCAL_POOL.with(LocalPool::acquire_node)
}

unsafe fn release_node(node: *mut WakerNode) {
    LOCAL_POOL.with(|lp| unsafe { LocalPool::release_node(lp, node) })
}

unsafe fn release_list(head: *mut WakerNode) {
    LOCAL_POOL.with(|lp| unsafe { LocalPool::release_list(lp, head) })
}

#[derive(Debug)]
pub struct WakerList {
    head: *mut WakerNode,
}

// It's okay to release the nodes onto some other thread.
unsafe impl Send for WakerList {}

impl Drop for WakerList {
    fn drop(&mut self) {
        unsafe {
            // Deallocate the individual wakers.
            // It's unfortunate to make two passes through the list.
            let mut p = self.head;
            while !p.is_null() {
                (*p).waker.assume_init_drop();
                p = (*p).next;
            }
            release_list(self.head)
        }
    }
}

impl Default for WakerList {
    fn default() -> Self {
        WakerList::new()
    }
}

impl WakerList {
    pub const fn new() -> WakerList {
        Self {
            head: ptr::null_mut(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }

    pub fn push(&mut self, waker: Waker) {
        let node = acquire_node();
        unsafe {
            (*node).waker.write(waker);
            (*node).next = self.head;
        }
        self.head = node;
    }

    pub fn pop(&mut self) -> Option<Waker> {
        if self.head.is_null() {
            None
        } else {
            Some(unsafe {
                let node = self.head;

                self.head = (*node).next;
                let waker = (*node).waker.assume_init_read();
                release_node(node);
                waker
            })
        }
    }
}
