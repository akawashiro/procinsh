#![allow(dead_code, unused_imports)]

mod api {
    mod implementation {
        pub async fn run<'a, T: AsRef<str>, const N: usize>(
            value: &'a T,
            bytes: [u8; N],
            callback: for<'b> unsafe extern "C" fn(&'b str) -> usize,
        ) -> Result<(&'a str, [u8; N]), &'static str>
        where
            T: Send + Sync,
        {
            Ok((value.as_ref(), bytes))
        }
        pub fn stream() -> impl Iterator<Item = u8> { [1].into_iter() }
        pub fn object(value: &(dyn std::fmt::Display + Send)) -> String { value.to_string() }
        pub fn associated<T: Iterator>(value: T::Item) -> T::Item { value }
        pub fn opaque(value: impl AsRef<str>) -> usize { value.as_ref().len() }
        pub struct Record<'a> { pub value: &'a str, private: [u8; 4] }
        pub struct Tuple(pub u8, pub bool);
        pub struct Unit;
        pub enum Event { Empty, Pair(u8, bool), Named { value: String } }
        pub type Alias = Option<(u8,)>;
        pub const MARKUP: &str = "<script>alert('x')</script>";
        pub(crate) fn restricted() {}
    }
    pub use implementation::{run as renamed_run, stream, object, associated, opaque, Record, Tuple, Unit, Event, Alias, MARKUP};
    // Neither ordinary imports nor restricted re-exports survive rustdoc's import stripping.
    use implementation::run;
    pub(super) use implementation::restricted;
    mod nested {
        pub use super::renamed_run as chained_run;
    }
}
