//! Port of upstream `trade/RateLimiter.ts` and the rate-limit parts of `trade/common.ts`.
//!
//! A [`RateLimiter`] keeps a "stack" of borrowed slots, each released `window`
//! seconds after it was taken. Waiters queue while all `max` slots are taken.
//! [`RateLimitRules`] is a set of limiters for one API policy (search, fetch,
//! exchange) that is re-synchronised with the server's `x-rate-limit-*`
//! headers after every response.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::Notify;

use crate::TradeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("RateLimiter is no longer active")]
pub struct LimiterDestroyed;

#[derive(Debug, Default)]
struct Inner {
    /// release times of borrowed slots, ascending
    stack: Vec<Instant>,
    queue: u32,
    destroyed: bool,
}

#[derive(Debug)]
pub struct RateLimiter {
    pub max: u32,
    /// seconds
    pub window: f64,
    inner: Mutex<Inner>,
    destroyed_notify: Notify,
}

impl RateLimiter {
    pub fn new(max: u32, window: f64) -> Self {
        Self { max, window, inner: Mutex::new(Inner::default()), destroyed_notify: Notify::new() }
    }

    fn window_dur(&self) -> Duration {
        Duration::from_secs_f64(self.window.max(0.0))
    }

    fn purge(inner: &mut Inner, now: Instant) {
        inner.stack.retain(|t| *t > now);
    }

    /// Borrow a slot immediately if one is free. Returns `false` if fully utilized.
    fn try_push(&self) -> bool {
        let mut inner = self.inner.lock();
        let now = Instant::now();
        Self::purge(&mut inner, now);
        if inner.stack.len() as u32 >= self.max {
            return false;
        }
        inner.stack.push(now + self.window_dur());
        true
    }

    /// Upstream `wait(borrow)`: waits until the limiter is not fully utilized,
    /// then (if `borrow`) takes a slot.
    pub async fn wait(&self, borrow: bool) -> Result<(), LimiterDestroyed> {
        loop {
            let until = {
                let mut inner = self.inner.lock();
                if inner.destroyed {
                    return Err(LimiterDestroyed);
                }
                let now = Instant::now();
                Self::purge(&mut inner, now);
                if (inner.stack.len() as u32) < self.max {
                    if borrow {
                        inner.stack.push(now + self.window_dur());
                    }
                    return Ok(());
                }
                inner.queue += 1;
                inner.stack[0]
            };
            let notified = self.destroyed_notify.notified();
            tokio::select! {
                _ = tokio::time::sleep_until(until.into()) => {}
                _ = notified => {}
            }
            self.inner.lock().queue -= 1;
        }
    }

    pub fn is_equal_limit(&self, max: u32, window: f64) -> bool {
        self.max == max && self.window == window
    }

    pub fn available(&self) -> u32 {
        let mut inner = self.inner.lock();
        Self::purge(&mut inner, Instant::now());
        self.max.saturating_sub(inner.stack.len() as u32)
    }

    pub fn is_fully_utilized(&self) -> bool {
        self.available() == 0
    }

    /// Number of currently borrowed slots (upstream `stack.length`).
    pub fn active(&self) -> u32 {
        let mut inner = self.inner.lock();
        Self::purge(&mut inner, Instant::now());
        inner.stack.len() as u32
    }

    pub fn queue(&self) -> u32 {
        self.inner.lock().queue
    }

    pub fn destroy(&self) {
        self.inner.lock().destroyed = true;
        // shortcircuit awaiters in queue
        self.destroyed_notify.notify_waiters();
    }

    /// Release times relative to now, in milliseconds, ascending.
    fn stack_millis(&self, now: Instant) -> Vec<f64> {
        let mut inner = self.inner.lock();
        Self::purge(&mut inner, now);
        let mut v: Vec<f64> = inner.stack.iter().map(|t| (*t - now).as_secs_f64() * 1000.0).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// Upstream `RateLimiter.estimateTime`, in milliseconds.
    pub fn estimate_time(count: u32, limiters: &[Arc<RateLimiter>], ignore_state: bool) -> f64 {
        // NOTE: Cannot handle existing queue in simulation, because
        //       entries in queue can depend on other limiters in `waitMulti` call.
        struct Sim {
            max: usize,
            window: f64,
            stack: Vec<f64>,
        }
        let now = Instant::now();
        let mut simulation: Vec<Sim> = limiters
            .iter()
            .map(|l| Sim {
                max: l.max as usize,
                window: l.window,
                stack: if ignore_state { Vec::new() } else { l.stack_millis(now) },
            })
            .collect();

        let mut total = 0.0f64;
        for _ in 0..count {
            while simulation.iter().any(|l| l.stack.len() >= l.max) {
                let wait_time = simulation.iter().fold(0.0f64, |ms, l| {
                    if l.stack.len() >= l.max { (l.stack.first().copied().unwrap_or(0.0) - total).max(ms) } else { ms }
                });
                total += wait_time;
                for l in &mut simulation {
                    l.stack.retain(|t| *t > total);
                }
                if wait_time <= 0.0 && simulation.iter().any(|l| l.max == 0) {
                    // a limiter with max=0 can never be satisfied; avoid an infinite loop
                    return f64::INFINITY;
                }
            }
            for l in &mut simulation {
                l.stack.push(total + l.window * 1000.0);
            }
        }
        total
    }
}

impl std::fmt::Display for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RateLimiter<max={}:window={}>: (stack={},queue={})",
            self.max,
            self.window,
            self.active(),
            self.queue()
        )
    }
}

// ---------------------------------------------------------------------------

/// One server rule parsed from `x-rate-limit-*` headers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerLimit {
    pub max: u32,
    /// seconds, including the client's `apiLatencySeconds` desync fix
    pub window: f64,
    /// current hits on the server
    pub state: u32,
}

/// Parses `x-rate-limit-rules` and the related headers. `get` returns a
/// header value by (lowercase) name. Returns `None` when the rules header is absent.
pub fn parse_rate_limit_headers(
    get: impl Fn(&str) -> Option<String>,
    desync_fix: f64,
) -> Option<Vec<ServerLimit>> {
    let rules_header = get("x-rate-limit-rules")?;
    let rules: Vec<String> = rules_header.split(',').map(|r| r.trim().to_lowercase()).collect();

    let limit_str = rules
        .iter()
        .map(|r| get(&format!("x-rate-limit-{r}")).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",");
    let state_str = rules
        .iter()
        .map(|r| get(&format!("x-rate-limit-{r}-state")).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",");

    let num = |s: Option<&str>| s.and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(f64::NAN);
    let limit_rule_state: Vec<f64> = state_str.split(',').map(|r| num(r.split(':').next())).collect();
    Some(
        limit_str
            .split(',')
            .enumerate()
            .map(|(idx, rule)| {
                let mut parts = rule.split(':');
                let max = num(parts.next());
                let window = num(parts.next());
                let state = limit_rule_state.get(idx).copied().unwrap_or(f64::NAN);
                ServerLimit {
                    max: if max.is_finite() { max as u32 } else { 0 },
                    window: window + desync_fix,
                    state: if state.is_finite() { state as u32 } else { 0 },
                }
            })
            .collect(),
    )
}

/// A set of limiters for one policy (upstream `RATE_LIMIT_RULES.SEARCH` etc.).
#[derive(Debug)]
pub struct RateLimitRules {
    pub policy: &'static str,
    limiters: Mutex<Vec<Arc<RateLimiter>>>,
}

impl RateLimitRules {
    pub fn new(policy: &'static str, initial: Vec<RateLimiter>) -> Self {
        Self { policy, limiters: Mutex::new(initial.into_iter().map(Arc::new).collect()) }
    }

    pub fn snapshot(&self) -> Vec<Arc<RateLimiter>> {
        self.limiters.lock().clone()
    }

    /// Upstream `RateLimiter.waitMulti`.
    pub async fn wait_multi(&self) {
        loop {
            let limiters = self.snapshot();
            let mut destroyed = false;
            for rl in &limiters {
                if rl.wait(false).await.is_err() {
                    destroyed = true;
                    break;
                }
            }
            if destroyed {
                continue;
            }
            if limiters.iter().all(|rl| !rl.is_fully_utilized()) {
                for rl in &limiters {
                    rl.try_push();
                }
                return;
            }
        }
    }

    /// Upstream `adjustRateLimits`/`_adjustRateLimits`.
    pub fn adjust(&self, server: &[ServerLimit]) {
        let mut limiters = self.limiters.lock();

        // destroy
        limiters.retain(|limit| {
            let active = server.iter().any(|s| limit.is_equal_limit(s.max, s.window));
            if !active {
                limit.destroy();
                tracing::debug!("Destroy {limit}");
            }
            active
        });

        // compare client<>server state
        for limit in limiters.iter() {
            let Some(server_limit) = server.iter().find(|s| limit.is_equal_limit(s.max, s.window)) else {
                continue;
            };
            let delta = server_limit.state as i64 - limit.active() as i64;
            if delta > 0 {
                tracing::debug!("Rate limit state on Server is greater by {delta}. Bursting to prevent rate limiting.");
                let n = (delta as u32).min(limit.available());
                for _ in 0..n {
                    limit.try_push();
                }
            } else if delta < 0 {
                tracing::debug!("Rate limit state on Client is greater by {}", -delta);
            }
        }

        // add new
        for server_limit in server {
            if limiters.iter().any(|l| l.is_equal_limit(server_limit.max, server_limit.window)) {
                continue;
            }
            let rl = Arc::new(RateLimiter::new(server_limit.max, server_limit.window));
            // NOTE: upstream queues `wait()` calls beyond `max`; those can only
            // extend an already exhausted window, we just fill the available slots.
            for _ in 0..server_limit.state.min(server_limit.max) {
                rl.try_push();
            }
            tracing::debug!("Add {rl}");
            limiters.push(rl);
        }
    }

    pub fn adjust_from_headers(&self, headers: &reqwest::header::HeaderMap, desync_fix: f64) {
        let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        if let Some(server) = parse_rate_limit_headers(get, desync_fix) {
            self.adjust(&server);
        }
    }

    pub fn state(&self) -> RateLimitPolicyState {
        let limiters = self.snapshot();
        let rules: Vec<RateLimitRuleState> = limiters
            .iter()
            .map(|rl| RateLimitRuleState { max: rl.max, window: rl.window, active: rl.active(), queue: rl.queue() })
            .collect();
        RateLimitPolicyState { policy: self.policy, has_queue: rules.iter().any(|r| r.queue > 0), rules }
    }
}

/// Upstream `preventQueueCreation`: fails with [`TradeError::RateLimited`] if
/// the request would have to wait >= 1.5s in the client queue.
pub fn prevent_queue_creation(targets: &[(u32, &RateLimitRules)]) -> Result<(), TradeError> {
    let estimated_millis = targets
        .iter()
        .map(|(count, rules)| {
            let limiters = rules.snapshot();
            let estimated = RateLimiter::estimate_time(*count, &limiters, false);
            let estimated_clean = RateLimiter::estimate_time(*count, &limiters, true);
            // ignore if impossible to run without queue
            if estimated == estimated_clean { 0.0 } else { estimated }
        })
        .fold(f64::NEG_INFINITY, f64::max);

    if estimated_millis >= 1500.0 {
        return Err(TradeError::RateLimited {
            retry_after_secs: crate::js::math_round(estimated_millis / 1000.0) as u64,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Observable state for the UI (upstream `RateLimiterState.vue`).

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitRuleState {
    pub max: u32,
    pub window: f64,
    pub active: u32,
    pub queue: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPolicyState {
    pub policy: &'static str,
    pub has_queue: bool,
    pub rules: Vec<RateLimitRuleState>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitState {
    pub limits: Vec<RateLimitPolicyState>,
}

impl RateLimitState {
    pub fn is_limited(&self) -> bool {
        self.limits.iter().any(|l| l.has_queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_headers() {
        let headers = [
            ("x-rate-limit-rules", "Account,Ip"),
            ("x-rate-limit-account", "3:5:60"),
            ("x-rate-limit-account-state", "1:5:0"),
            ("x-rate-limit-ip", "8:10:60,15:60:120"),
            ("x-rate-limit-ip-state", "2:10:0,7:60:0"),
        ];
        let get = |n: &str| headers.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string());
        let parsed = parse_rate_limit_headers(get, 0.5).unwrap();
        assert_eq!(
            parsed,
            vec![
                ServerLimit { max: 3, window: 5.5, state: 1 },
                ServerLimit { max: 8, window: 10.5, state: 2 },
                ServerLimit { max: 15, window: 60.5, state: 7 },
            ]
        );
        assert!(parse_rate_limit_headers(|_| None, 0.0).is_none());
    }

    #[test]
    fn adjust_and_estimate() {
        let rules = RateLimitRules::new("trade-search-request-limit", vec![RateLimiter::new(1, 5.0)]);
        rules.adjust(&[ServerLimit { max: 3, window: 5.0, state: 3 }, ServerLimit { max: 10, window: 60.0, state: 1 }]);
        let state = rules.state();
        assert_eq!(state.rules.len(), 2);
        assert_eq!(state.rules[0], RateLimitRuleState { max: 3, window: 5.0, active: 3, queue: 0 });
        assert_eq!(state.rules[1].active, 1);

        // the 3:5 limiter is full, next request needs ~5s
        let est = RateLimiter::estimate_time(1, &rules.snapshot(), false);
        assert!(est > 4000.0 && est <= 5000.0, "{est}");
        assert_eq!(RateLimiter::estimate_time(1, &rules.snapshot(), true), 0.0);
        let err = prevent_queue_creation(&[(1, &rules)]).unwrap_err();
        assert!(matches!(err, TradeError::RateLimited { retry_after_secs: 5 }), "{err:?}");

        // server state lower than client: nothing changes; same rules keep their limiter
        rules.adjust(&[ServerLimit { max: 3, window: 5.0, state: 0 }, ServerLimit { max: 10, window: 60.0, state: 2 }]);
        let state = rules.state();
        assert_eq!(state.rules[0].active, 3);
        assert_eq!(state.rules[1].active, 2);

        // removed rule gets destroyed
        rules.adjust(&[ServerLimit { max: 10, window: 60.0, state: 2 }]);
        assert_eq!(rules.state().rules.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn waits_for_window() {
        let rules = RateLimitRules::new("p", vec![RateLimiter::new(1, 2.0)]);
        let start = tokio::time::Instant::now();
        rules.wait_multi().await;
        assert!(start.elapsed() < Duration::from_millis(10));
        assert!(rules.snapshot()[0].is_fully_utilized());
    }
}
