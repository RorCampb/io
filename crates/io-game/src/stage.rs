//! Typed synchronous data flow. Stages do not imply threads, queues or world ownership.

/// A developer defines the request, result, failure and any borrowed context.
/// Input is generic so outputs can retain the input's lifetime without copying.
/// No Send/Sync requirement is imposed; worker boundaries enforce those separately.
pub trait Stage<Input> {
    type Output;
    type Error;

    fn run(&mut self, input: Input) -> Result<Self::Output, Self::Error>;

    fn then<Next>(self, next: Next) -> Then<Self, Next>
    where
        Self: Sized,
        Next: Stage<Self::Output, Error = Self::Error>,
    {
        Then(self, next)
    }
}

/// Only compatible result/request types and explicit common errors compose.
///
/// ```compile_fail
/// use io_game::stage::Stage;
/// struct Number;
/// impl Stage<()> for Number {
///     type Output = u32;
///     type Error = ();
///     fn run(&mut self, _: ()) -> Result<u32, ()> { Ok(1) }
/// }
/// struct Text;
/// impl Stage<String> for Text {
///     type Output = usize;
///     type Error = ();
///     fn run(&mut self, s: String) -> Result<usize, ()> { Ok(s.len()) }
/// }
/// let _ = Number.then(Text); // u32 is not Text's input contract
/// ```
pub struct Then<A, B>(pub A, pub B);

impl<Input, A, B> Stage<Input> for Then<A, B>
where
    A: Stage<Input>,
    B: Stage<A::Output, Error = A::Error>,
{
    type Output = B::Output;
    type Error = A::Error;

    fn run(&mut self, input: Input) -> Result<Self::Output, Self::Error> {
        let result = self.0.run(input)?;
        self.1.run(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Parse;
    struct Double(usize);
    impl Stage<&str> for Parse {
        type Output = u32;
        type Error = &'static str;
        fn run(&mut self, input: &str) -> Result<u32, Self::Error> {
            input.parse().map_err(|_| "invalid number")
        }
    }
    impl Stage<u32> for Double {
        type Output = u32;
        type Error = &'static str;
        fn run(&mut self, input: u32) -> Result<u32, Self::Error> {
            self.0 += 1;
            input.checked_mul(2).ok_or("overflow")
        }
    }
    #[test]
    fn typed_results_flow_forward_and_failure_stops_downstream_work() {
        let mut pipeline = Parse.then(Double(0));
        assert_eq!(pipeline.run("21"), Ok(42));
        assert_eq!(pipeline.run("bad"), Err("invalid number"));
        assert_eq!(pipeline.1 .0, 1);
    }
}
