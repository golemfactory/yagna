fn get_innermost_backtrace_symbol(fm: &backtrace::BacktraceFrame) -> Option<String> {
    for symbol in fm.symbols().iter() {
        if let Some(name) = symbol.name() {
            return Some(name.to_string());
        }
    }
    None
}

fn adjust_backtrace_level(frames: &[backtrace::BacktraceFrame]) -> Option<usize> {
    // On some systems backtrace lib doesn't properly set actual_start_index
    for (idx, frame) in frames.iter().enumerate() {
        if let Some(name) = get_innermost_backtrace_symbol(frame) {
            // Note: On windows there is no "::<hash>" suffix
            if name.starts_with("ya_market::testing::backtrace_util::generate_backtraced_name") {
                return Some(idx);
            }
        }
    }
    None
}

fn get_symbol_at_level(bt: &backtrace::Backtrace, lvl: usize) -> Option<String> {
    let frames = bt.frames();
    match adjust_backtrace_level(frames) {
        Some(adjustment) => {
            let frame = &frames[lvl + adjustment];
            return get_innermost_backtrace_symbol(frame);
        }
        _ => {
            log::trace!("Cannot find adjustment for symbol. lvl={}", lvl);
        }
    };
    None
}

pub fn generate_backtraced_name(level: Option<usize>) -> Option<String> {
    let bt = backtrace::Backtrace::new();

    // 0th element should be this function. We'd like to know the caller
    let name = get_symbol_at_level(&bt, level.unwrap_or(1));

    if let Some(name) = &name {
        log::trace!("Generated name: {} level: {:?} BT: {:#?}", name, level, bt);
    } else {
        log::warn!("No backtrace support. bt={:#?}", bt);
    }

    name
}
