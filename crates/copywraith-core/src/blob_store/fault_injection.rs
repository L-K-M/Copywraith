//! Explicit test-only feature; hooks are thread-local and never enabled by apps.
use std::cell::RefCell;
use std::io;
use std::marker::PhantomData;
use std::rc::Rc;

pub use super::durable_file::Boundary;

type Hook = Box<dyn FnMut(Boundary) -> io::Result<()>>;
thread_local! { static HOOK: RefCell<Option<Hook>> = RefCell::new(None); }

// Cleanup must run on the thread that owns the hook.
pub struct Guard(PhantomData<Rc<()>>);

pub fn install(hook: impl FnMut(Boundary) -> io::Result<()> + 'static) -> Guard {
    HOOK.with(|slot| {
        assert!(slot.borrow().is_none(), "Blob fault hook already installed");
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    Guard(PhantomData)
}

pub(super) fn check(point: Boundary) -> io::Result<()> {
    HOOK.with(|slot| match slot.borrow_mut().as_mut() {
        Some(hook) => hook(point),
        None => Ok(()),
    })
}

impl Drop for Guard {
    fn drop(&mut self) {
        HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}
