use actix_web::Scope;

pub trait ExtendableScope {
    fn extend<F>(self, f: F) -> Self
    where
        Self: Sized,
        F: FnOnce(Self) -> Self;
}

// impl ExtendableScope for Scope {
//     #[inline(always)]
//     fn extend<F>(self, f: F) -> Self
//     where
//         Self: Sized,
//         F: FnOnce(Self) -> Self,
//     {
//         f(self)
//     }
// }

impl<T> ExtendableScope for Scope<T> {
    #[inline(always)]
    fn extend<F>(self, f: F) -> Self
    where
        Self: Sized,
        F: FnOnce(Self) -> Self,
    {
        f(self)
    }
}
