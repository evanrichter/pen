use super::{async_stack_action::AsyncStackAction, CpsError, Stack};
use alloc::{vec, vec::Vec};
use core::{
    future::Future,
    intrinsics::transmute,
    ops::{Deref, DerefMut},
    task::Context,
};

pub type StepFunction<T, V = ()> =
    fn(stack: &mut AsyncStack<V>, continuation: ContinuationFunction<T, V>);

pub type ContinuationFunction<T, V = ()> = extern "C" fn(&mut AsyncStack<V>, T);

pub type Trampoline<T, V> = (StepFunction<T, V>, ContinuationFunction<T, V>);

// Something like AsyncStackManager that creates async stacks with proper
// lifetime every time when it's given contexts can be implemented potentially.
// The reason we set those contexts unsafely with the `run_with_context` method
// in the current implementation is to keep struct type compatible between
// Stack and AsyncStack at the ABI level.
#[repr(C)]
#[derive(Debug)]
pub struct AsyncStack<V = ()> {
    stack: Stack,
    context: Option<*mut Context<'static>>,
    // This field is currently used only for validation in FFI but not by codes
    // generated by the compiler.
    next_actions: Vec<AsyncStackAction>,
    resolved_value: Option<V>,
}

impl<V> AsyncStack<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            stack: Stack::new(capacity),
            context: None,
            next_actions: vec![AsyncStackAction::Suspend],
            resolved_value: None,
        }
    }

    pub fn context(&mut self) -> Option<&mut Context<'_>> {
        self.context
            .map(|context| unsafe { &mut *transmute::<_, *mut Context<'_>>(context) })
    }

    pub fn run_with_context<T>(
        &mut self,
        context: &mut Context<'_>,
        callback: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.context = Some(unsafe { transmute(context) });

        let value = callback(self);

        self.context = None;

        value
    }

    pub fn suspend<T>(
        &mut self,
        step: StepFunction<T, V>,
        continuation: ContinuationFunction<T, V>,
        future: impl Future + Unpin,
    ) -> Result<(), CpsError> {
        self.validate_action(AsyncStackAction::Suspend)?;
        self.push_next_actions(&[
            AsyncStackAction::Resume,
            AsyncStackAction::Restore,
            AsyncStackAction::Suspend,
        ]);

        self.stack.push(future);
        self.stack.push(step);
        self.stack.push(continuation);

        Ok(())
    }

    // Trampoline a continuation function call to call it from the near bottom
    // of stack clearing the current stack frames.
    // Without this due to the lack of tail call elimination in Rust, machine
    // stacks can grow arbitrarily deep.
    // https://github.com/rust-lang/rfcs/issues/2691
    pub fn trampoline<T>(
        &mut self,
        continuation: ContinuationFunction<T, V>,
        value: T,
    ) -> Result<(), CpsError> {
        self.validate_action(AsyncStackAction::Suspend)?;
        self.push_next_actions(&[AsyncStackAction::Resume, AsyncStackAction::Suspend]);

        fn step<T, V>(stack: &mut AsyncStack<V>, continue_: ContinuationFunction<T, V>) {
            let value = stack.pop::<T>();

            continue_(stack, value)
        }

        let step: StepFunction<T, V> = step;

        self.stack.push(value);
        self.stack.push(step);
        self.stack.push(continuation);

        self.context()
            .ok_or(CpsError::MissingContext)?
            .waker()
            .wake_by_ref();

        Ok(())
    }

    pub fn resume<T>(&mut self) -> Result<Trampoline<T, V>, CpsError> {
        self.validate_action(AsyncStackAction::Resume)?;

        let continuation = self.pop();
        let step = self.pop();

        Ok((step, continuation))
    }

    pub fn restore<F: Future + Unpin>(&mut self) -> Result<F, CpsError> {
        self.validate_action(AsyncStackAction::Restore)?;

        Ok(self.pop())
    }

    pub fn resolved_value(&mut self) -> Option<V> {
        self.resolved_value.take()
    }

    pub fn resolve(&mut self, value: V) {
        self.resolved_value = Some(value);
    }

    fn validate_action(&mut self, current_action: AsyncStackAction) -> Result<(), CpsError> {
        let next_action = self.next_actions.pop();

        if next_action != Some(current_action) {
            return Err(CpsError::UnexpectedAsyncStackAction(next_action));
        }

        Ok(())
    }

    fn push_next_actions(&mut self, next_actions: &[AsyncStackAction]) {
        self.next_actions.extend(next_actions.iter().rev().copied());
    }
}

impl<V> Deref for AsyncStack<V> {
    type Target = Stack;

    fn deref(&self) -> &Stack {
        &self.stack
    }
}

impl<V> DerefMut for AsyncStack<V> {
    fn deref_mut(&mut self) -> &mut Stack {
        &mut self.stack
    }
}

// We can mark async stacks Send + Send because:
//
// - Stack should implement Send + Sync. Currently, we don't as we don't need
//   to.
// - Option<*mut Context> is cleared to None on every non-preemptive run.
unsafe impl<V: Send> Send for AsyncStack<V> {}

unsafe impl<V: Sync> Sync for AsyncStack<V> {}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::{ready, Ready},
        ptr::null,
        task::{RawWaker, RawWakerVTable, Waker},
    };

    const TEST_CAPACITY: usize = 1;
    const RAW_WAKER_DATA: () = ();
    const RAW_WAKER_V_TABLE: RawWakerVTable =
        RawWakerVTable::new(clone_waker, do_nothing, do_nothing, do_nothing);

    fn create_waker() -> Waker {
        unsafe { Waker::from_raw(RawWaker::new(&RAW_WAKER_DATA, &RAW_WAKER_V_TABLE)) }
    }

    fn clone_waker(_: *const ()) -> RawWaker {
        RawWaker::new(null(), &RAW_WAKER_V_TABLE)
    }

    fn do_nothing(_: *const ()) {}

    type TestResult = usize;

    fn step(_: &mut AsyncStack, _: ContinuationFunction<TestResult, ()>) {}

    extern "C" fn continue_(_: &mut AsyncStack, _: TestResult) {}

    #[allow(dead_code)]
    extern "C" {
        fn _test_async_stack_ffi_safety(_: &mut Stack);
    }

    #[test]
    fn push_f64() {
        let mut stack = AsyncStack::<()>::new(TEST_CAPACITY);

        stack.push(42.0f64);

        assert_eq!(stack.pop::<f64>(), 42.0);
    }

    #[test]
    fn wake() {
        let waker = create_waker();
        let mut stack = AsyncStack::<()>::new(TEST_CAPACITY);
        let mut context = Context::from_waker(&waker);

        stack.run_with_context(&mut context, |stack| {
            stack.context().unwrap().waker().wake_by_ref()
        });
    }

    #[test]
    fn suspend() {
        let mut stack = AsyncStack::new(TEST_CAPACITY);

        stack.suspend(step, continue_, ready(42)).unwrap();
    }

    #[tokio::test]
    async fn suspend_and_resume() {
        let mut stack = AsyncStack::new(TEST_CAPACITY);

        type TestFuture = Ready<usize>;

        let future: TestFuture = ready(42);

        stack.suspend(step, continue_, future).unwrap();
        stack.resume::<()>().unwrap();
        assert_eq!(stack.restore::<TestFuture>().unwrap().await, 42);
    }

    #[tokio::test]
    async fn fail_to_restore_before_resume() {
        let mut stack = AsyncStack::new(TEST_CAPACITY);

        type TestFuture = Ready<()>;

        let future: TestFuture = ready(());

        stack.suspend(step, continue_, future).unwrap();
        assert_eq!(
            stack.restore::<TestFuture>().unwrap_err(),
            CpsError::UnexpectedAsyncStackAction(Some(AsyncStackAction::Resume))
        );
    }

    #[tokio::test]
    async fn trampoline_and_resume() {
        type Stack = AsyncStack<usize>;

        let mut stack = Stack::new(TEST_CAPACITY);

        extern "C" fn continue_(stack: &mut Stack, value: usize) {
            stack.resolve(value);
        }

        let waker = create_waker();
        let mut context = Context::from_waker(&waker);

        stack.run_with_context(&mut context, |stack| {
            stack.trampoline(continue_, 42).unwrap();
        });

        let (step, continue_) = stack.resume::<()>().unwrap();

        stack.run_with_context(&mut context, |stack| {
            step(stack, continue_);
        });

        assert_eq!(stack.resolved_value(), Some(42));
    }
}
