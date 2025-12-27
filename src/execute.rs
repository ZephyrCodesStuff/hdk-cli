use enum_dispatch::enum_dispatch;

#[enum_dispatch]
pub trait Execute {
    fn execute(self);
}
