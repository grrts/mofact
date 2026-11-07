//! The full game (T61). The demo stops research after tier
//! [`DEMO_TIER`]; one purchase opens the rest and the skyhook.
//!
//! A platform sells it through a [`Shop`]: RevenueCat on phones, and on
//! desktop and the web a [`Sandbox`] that unlocks without payment until a
//! real store is set up. Whether it was bought is remembered on the device
//! as soon as the shop says so, so the full game opens offline and nothing
//! waits on the network (T64).

/// The demo's last open tier.
pub const DEMO_TIER: u8 = 4;

/// The entitlement RevenueCat grants for the full game.
pub const ENTITLEMENT: &str = "full_game";

/// What a shop has to tell the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The full game's price, as the store words it.
    Price(String),
    /// The player owns the full game: bought now, restored, or found on
    /// the account at start.
    Owned,
    /// The player backed out of the purchase.
    Cancelled,
    /// A restore found nothing bought on this account.
    NothingToRestore,
    /// The store couldn't be reached or refused; the words say why.
    Failed(String),
}

/// A store that sells the full game.
pub trait Shop {
    /// Whether the full game was bought, as last remembered on this device.
    /// Never touches the network.
    fn remembered(&self) -> bool;
    /// Start buying the full game; the answer comes from [`Shop::poll`].
    fn buy(&mut self);
    /// Look for the full game bought before on this account.
    fn restore(&mut self);
    /// What has happened since the last call.
    fn poll(&mut self) -> Vec<Event>;
}

/// The purchase as the game sees it.
pub struct Purchase {
    shop: Box<dyn Shop>,
    owned: bool,
    price: Option<String>,
    busy: bool,
    note: Option<String>,
}

impl Purchase {
    pub fn new(shop: Box<dyn Shop>) -> Self {
        Self {
            owned: shop.remembered(),
            shop,
            price: None,
            busy: false,
            note: None,
        }
    }

    pub fn owned(&self) -> bool {
        self.owned
    }

    /// The gate the simulation should have: none once the game is owned.
    pub fn demo(&self) -> Option<u8> {
        (!self.owned).then_some(DEMO_TIER)
    }

    pub fn buy(&mut self) {
        if !self.owned && !self.busy {
            self.busy = true;
            self.note = None;
            self.shop.buy();
        }
    }

    pub fn restore(&mut self) {
        if !self.owned && !self.busy {
            self.busy = true;
            self.note = None;
            self.shop.restore();
        }
    }

    /// Take in what the shop says. Returns true when the full game has
    /// just opened.
    pub fn poll(&mut self) -> bool {
        let mut opened = false;
        for event in self.shop.poll() {
            match event {
                Event::Price(p) => self.price = Some(p),
                Event::Owned => {
                    opened |= !self.owned;
                    self.owned = true;
                    self.busy = false;
                    self.note = None;
                }
                Event::Cancelled => {
                    self.busy = false;
                    self.note = Some("Purchase cancelled: nothing was charged.".into());
                }
                Event::NothingToRestore => {
                    self.busy = false;
                    self.note = Some("No purchase found on this account.".into());
                }
                Event::Failed(why) => {
                    self.busy = false;
                    self.note = Some(why);
                }
            }
        }
        opened
    }

    pub fn view(&self) -> mofact_ui::StoreView {
        mofact_ui::StoreView {
            demo: self.demo(),
            price: self.price.clone(),
            busy: self.busy,
            note: self.note.clone(),
        }
    }
}

/// A shop with no store behind it: buying unlocks at once and costs
/// nothing. `keep` remembers the unlock wherever the platform keeps such
/// things.
pub struct Sandbox {
    owned: bool,
    keep: Box<dyn FnMut(bool)>,
    events: Vec<Event>,
}

impl Sandbox {
    pub fn new(owned: bool, keep: Box<dyn FnMut(bool)>) -> Self {
        Self {
            owned,
            keep,
            events: vec![Event::Price("test, free".into())],
        }
    }
}

impl Shop for Sandbox {
    fn remembered(&self) -> bool {
        self.owned
    }

    fn buy(&mut self) {
        self.owned = true;
        (self.keep)(true);
        self.events.push(Event::Owned);
    }

    fn restore(&mut self) {
        self.events.push(if self.owned {
            Event::Owned
        } else {
            Event::NothingToRestore
        });
    }

    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn buying_opens_the_full_game_and_is_remembered() {
        let kept = Rc::new(Cell::new(false));
        let k = kept.clone();
        let mut p = Purchase::new(Box::new(Sandbox::new(false, Box::new(move |o| k.set(o)))));
        assert_eq!(p.demo(), Some(DEMO_TIER));
        assert!(!p.poll());
        assert!(p.view().price.is_some());
        p.restore();
        assert!(!p.poll());
        assert_eq!(
            p.view().note.as_deref(),
            Some("No purchase found on this account.")
        );
        p.buy();
        assert!(p.view().busy);
        assert!(p.poll(), "the full game opens");
        assert_eq!(p.demo(), None);
        assert!(kept.get(), "and the device remembers it");
        // Next start, offline: the remembered unlock is enough.
        let p = Purchase::new(Box::new(Sandbox::new(kept.get(), Box::new(|_| {}))));
        assert_eq!(p.demo(), None);
    }

    /// A store that answers late, or not at all.
    struct Slow(Vec<Event>);

    impl Shop for Slow {
        fn remembered(&self) -> bool {
            false
        }
        fn buy(&mut self) {}
        fn restore(&mut self) {}
        fn poll(&mut self) -> Vec<Event> {
            std::mem::take(&mut self.0)
        }
    }

    #[test]
    fn a_store_that_never_answers_blocks_nothing_but_the_buy_button() {
        let mut p = Purchase::new(Box::new(Slow(Vec::new())));
        p.buy();
        for _ in 0..100 {
            assert!(!p.poll());
        }
        assert!(p.view().busy);
        assert_eq!(p.demo(), Some(DEMO_TIER), "the demo plays on");
        let mut p = Purchase::new(Box::new(Slow(vec![Event::Failed(
            "The store couldn't be reached.".into(),
        )])));
        p.buy();
        p.poll();
        assert!(!p.view().busy, "a failure frees the button");
    }
}
