//! The Play queue's order: repeat mode, the shuffle order, and which index follows which. Pure, so the
//! prefetch and the schedule cannot disagree about what is next.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

pub fn build_shuffle_order(length: usize, start: usize) -> Vec<usize> {
    let mut rest: Vec<usize> = (0..length).filter(|&i| i != start).collect();
    // Fisher-Yates, seeded from the clock (no rand dependency needed here).
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in (1..rest.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        rest.swap(i, j);
    }
    if start < length {
        let mut out = vec![start];
        out.extend(rest);
        out
    } else {
        rest
    }
}

/// Which queue index follows `index`, or None for "stop here". A pure
/// function of an already-decided order so the prefetch and the schedule
/// can't disagree about what's next.
pub fn resolve_next_index(index: usize, len: usize, order: Option<&Vec<usize>>, repeat: RepeatMode) -> Option<usize> {
    if repeat == RepeatMode::One || len == 0 {
        return None;
    }
    if let Some(order) = order {
        if order.len() == len {
            let pos = order.iter().position(|&i| i == index)?;
            if pos + 1 < order.len() {
                return Some(order[pos + 1]);
            }
            return if repeat == RepeatMode::All { Some(order[0]) } else { None };
        }
    }
    if index + 1 < len {
        return Some(index + 1);
    }
    if repeat == RepeatMode::All {
        Some(0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_is_a_permutation_starting_at_the_current_track() {
        let order = build_shuffle_order(20, 7);
        assert_eq!(order[0], 7);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn next_follows_the_queue_and_stops_or_wraps_at_the_end() {
        assert_eq!(resolve_next_index(0, 3, None, RepeatMode::Off), Some(1));
        assert_eq!(resolve_next_index(2, 3, None, RepeatMode::Off), None);
        assert_eq!(resolve_next_index(2, 3, None, RepeatMode::All), Some(0));
        assert_eq!(resolve_next_index(0, 3, None, RepeatMode::One), None);
        assert_eq!(resolve_next_index(0, 0, None, RepeatMode::All), None);
    }

    #[test]
    fn next_follows_the_shuffle_order_when_it_matches_the_queue() {
        let order = vec![2, 0, 1];
        assert_eq!(resolve_next_index(2, 3, Some(&order), RepeatMode::Off), Some(0));
        assert_eq!(resolve_next_index(1, 3, Some(&order), RepeatMode::Off), None);
        assert_eq!(resolve_next_index(1, 3, Some(&order), RepeatMode::All), Some(2));
        // A stale order (queue edited since) is ignored.
        assert_eq!(resolve_next_index(0, 4, Some(&order), RepeatMode::Off), Some(1));
    }
}
