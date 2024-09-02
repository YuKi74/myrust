#[macro_export]
macro_rules! build_state {
    ($vis:vis $state_name:ident $(,$member_vis:vis $member_name:ident: $member:ty)*) => {
        #[derive(Clone)]
        $vis struct $state_name {
            $($member_vis $member_name: $member,)*
        }
        impl<S> axum::extract::FromRef<S> for $state_name
        where
            S: AsRef<$state_name>,
        {
            fn from_ref(t: &S) -> Self {
                t.as_ref().clone()
            }
        }
        $(
        impl AsRef<$member> for $state_name {
            #[inline]
            fn as_ref(&self) -> &$member {
                &self.$member_name
            }
        }
        )*
    };
}
