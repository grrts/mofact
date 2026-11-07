//! The full game through RevenueCat (T61), by way of `MofactActivity`.
//!
//! The game calls the activity's `store*` methods over JNI; the activity
//! answers on its own thread through `nativeStoreEvent`, which queues the
//! answer here until the game's next frame reads it.

use mofact::shop::{Event, Shop};
use std::sync::Mutex;

/// Answers from the activity, oldest first.
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// Queue an answer from the activity. `kind` follows `MofactActivity`'s
/// constants.
pub fn answer(kind: i32, text: String) {
    let event = match kind {
        0 => Event::Price(text),
        1 => Event::Owned,
        2 => Event::Cancelled,
        3 => Event::NothingToRestore,
        // No words from the store: the game says it in the player's language.
        _ => Event::Failed(text),
    };
    EVENTS.lock().unwrap_or_else(|e| e.into_inner()).push(event);
}

/// Calls an activity method that takes nothing and answers true or false.
pub type Call = fn(&str) -> bool;

/// RevenueCat behind the activity.
pub struct RevenueCat {
    remembered: bool,
    keep: Box<dyn FnMut(bool)>,
    call: Call,
}

impl RevenueCat {
    pub fn new(remembered: bool, keep: Box<dyn FnMut(bool)>, call: Call) -> Self {
        Self {
            remembered,
            keep,
            call,
        }
    }
}

impl Shop for RevenueCat {
    fn remembered(&self) -> bool {
        self.remembered
    }

    fn buy(&mut self) {
        if !(self.call)("storeBuy") {
            answer(4, String::new());
        }
    }

    fn restore(&mut self) {
        if !(self.call)("storeRestore") {
            answer(4, String::new());
        }
    }

    fn poll(&mut self) -> Vec<Event> {
        let events = std::mem::take(&mut *EVENTS.lock().unwrap_or_else(|e| e.into_inner()));
        if events.contains(&Event::Owned) && !self.remembered {
            self.remembered = true;
            (self.keep)(true);
        }
        events
    }
}

/// The shop for this device: RevenueCat once the activity has a key,
/// otherwise the free sandbox.
pub fn make(call: Call) -> mofact::MakeShop {
    Box::new(move |owned, keep| {
        if call("storeStart") {
            Box::new(RevenueCat::new(owned, keep, call))
        } else {
            Box::new(mofact::shop::Sandbox::new(owned, keep))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn store(method: &str) -> bool {
        // A store that answers at once, as the activity does on its thread.
        match method {
            "storeStart" => true,
            "storeBuy" => {
                answer(0, "€4.99".into());
                answer(1, String::new());
                true
            }
            _ => false,
        }
    }

    #[test]
    fn answers_from_the_activity_reach_the_game_and_are_remembered() {
        let kept = Rc::new(Cell::new(false));
        let k = kept.clone();
        let mut shop = make(store)(false, Box::new(move |o| k.set(o)));
        shop.restore();
        assert_eq!(
            shop.poll(),
            vec![Event::Failed(String::new())],
            "a call the activity can't take is a failure, not a hang"
        );
        shop.buy();
        assert_eq!(
            shop.poll(),
            vec![Event::Price("€4.99".into()), Event::Owned]
        );
        assert!(kept.get());
        assert!(shop.poll().is_empty());
    }
}
