use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::{Add, Sub};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use num_traits::{Bounded, Saturating};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use ya_core_model::NodeId;

pub type NodeReputation = Reputation<NodeId, i64>;

#[derive(Clone, Default)]
pub struct Reputation<K, T>
where
    K: Eq + Hash,
{
    inner: Arc<RwLock<HashMap<K, Score<T>>>>,
}

impl<K, T> Reputation<K, T>
where
    K: Eq + Hash,
    T: Value,
{
    pub async fn get_score(&self, key: K) -> Score<T> {
        let inner = self.inner.read().await;
        let score = inner.get(&key).cloned().unwrap_or_else(Default::default);

        if score.is_past_due() {
            score.into_value()
        } else {
            score
        }
    }

    pub async fn update_score(&self, key: K, score: Score<T>) {
        let mut inner = self.inner.write().await;
        inner.insert(key, score);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Score<T> {
    Trusted {
        at: DateTime<Utc>,
        until: DateTime<Utc>,
        value: T,
    },
    Banned {
        at: DateTime<Utc>,
        until: DateTime<Utc>,
        value: T,
    },
    Value(T),
}

impl<T: Value> Score<T> {
    #[inline]
    pub fn is_trusted(&self) -> bool {
        if let Self::Trusted { .. } = self {
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn is_banned(&self) -> bool {
        if let Self::Banned { .. } = self {
            true
        } else {
            false
        }
    }

    pub fn is_past_due(&self) -> bool {
        match self {
            Self::Trusted { until, .. } => *until < Utc::now(),
            Self::Banned { until, .. } => *until < Utc::now(),
            Self::Value(_) => false,
        }
    }

    pub fn value(&self) -> T {
        match self {
            Self::Trusted { value, .. } => *value,
            Self::Banned { value, .. } => *value,
            Self::Value(value) => *value,
        }
    }

    pub fn into_trusted<D: Into<Duration>>(self, duration: D) -> Self {
        let at = Utc::now();
        let until = at + duration.into();
        let value = self.value();

        Self::Trusted { at, until, value }
    }

    pub fn into_banned<D: Into<Duration>>(self, duration: D) -> Self {
        let at = Utc::now();
        let until = at + duration.into();
        let value = self.value();

        Self::Banned { at, until, value }
    }

    #[inline]
    pub fn into_value(self) -> Self {
        Self::Value(self.value())
    }
}

impl<T: Value> Default for Score<T> {
    fn default() -> Self {
        Self::Value(T::default())
    }
}

impl<T: Value> Add<T> for Score<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self::Output {
        if !rhs.is_positive() {
            return self.sub(rhs);
        }

        let mut score = if self.is_past_due() {
            self.into_value()
        } else {
            self
        };

        match &mut score {
            Self::Trusted { value, .. } => *value = value.saturating_add(rhs),
            Self::Banned { value, .. } => *value = value.saturating_add(rhs),
            Self::Value(value) => *value = value.saturating_add(rhs),
        };
        score
    }
}

impl<T: Value> Sub<T> for Score<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self::Output {
        if !rhs.is_positive() {
            return self.add(rhs);
        }

        let mut score = if self.is_past_due() {
            self.into_value()
        } else {
            self
        };

        match &mut score {
            Self::Trusted { value, .. } => *value = value.saturating_sub(rhs),
            Self::Banned { value, .. } => *value = value.saturating_sub(rhs),
            Self::Value(value) => *value = value.saturating_sub(rhs),
        };
        score
    }
}

impl<T: Value> PartialEq for Score<T> {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other).unwrap() == Ordering::Equal
    }
}

impl<T: Value> Eq for Score<T> {}

impl<T: Value> PartialOrd for Score<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self {
            Self::Trusted { .. } => match other {
                Self::Trusted { .. } => Some(Ordering::Equal),
                _ => Some(Ordering::Greater),
            },
            Self::Banned { .. } => match other {
                Self::Banned { .. } => Some(Ordering::Equal),
                _ => Some(Ordering::Less),
            },
            Self::Value(v) => match other {
                Self::Trusted { .. } => Some(Ordering::Less),
                Self::Banned { .. } => Some(Ordering::Greater),
                Self::Value(o) => v.partial_cmp(o),
            },
        }
    }
}

impl<T: Value> Ord for Score<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub trait Value: Bounded + Saturating + PartialOrd + Copy + Default {
    fn is_positive(&self) -> bool;
}

macro_rules! impl_signed_value {
    ($ty:ty) => {
        impl Value for $ty {
            #[inline]
            fn is_positive(&self) -> bool {
                *self >= 0
            }
        }
    };
}

macro_rules! impl_unsigned_value {
    ($ty:ty) => {
        impl Value for $ty {
            #[inline]
            fn is_positive(&self) -> bool {
                true
            }
        }
    };
}

impl_signed_value!(i8);
impl_signed_value!(i16);
impl_signed_value!(i64);
impl_signed_value!(i32);
impl_signed_value!(i128);

impl_unsigned_value!(u8);
impl_unsigned_value!(u16);
impl_unsigned_value!(u32);
impl_unsigned_value!(u64);
impl_unsigned_value!(u128);
