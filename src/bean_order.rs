//! The bean (puyo) sequence for a match (`GenPuyoOrder`).
//!
//! At match start the game fills a 256-entry list by cycling through the bean colors,
//! then shuffles it with 256 random swaps. Both players draw from the same list, so
//! everyone gets the same pairs in the same order.

use super::rng::Rng;

pub const ORDER_LEN: usize = 256;

/// Bean color IDs, numbered as in the original (`PUYO_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BeanColor {
    Red = 0,
    Yellow = 1,
    /// Never dealt in Mean Bean Machine (left over from Puyo Puyo).
    Teal = 2,
    Green = 3,
    Purple = 4,
    Blue = 5,
    Garbage = 6,
}

impl BeanColor {
    pub const ALL: [BeanColor; 7] = [
        BeanColor::Red,
        BeanColor::Yellow,
        BeanColor::Teal,
        BeanColor::Green,
        BeanColor::Purple,
        BeanColor::Blue,
        BeanColor::Garbage,
    ];

    pub fn from_id(id: u8) -> Option<BeanColor> {
        Self::ALL.get(id as usize).copied()
    }
}

/// Colors in the order the list cycles through them (`PuyoColors`).
pub const COLOR_CYCLE: [BeanColor; 5] =
    [BeanColor::Red, BeanColor::Yellow, BeanColor::Green, BeanColor::Purple, BeanColor::Blue];

/// Colors dealt per stage (`MaxPuyoColors`): the first three stages leave out blue.
const COLORS_PER_LEVEL: [usize; 16] = [4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5];

/// `level_mode` value where player 2 gets a partly different sequence.
pub const LEVEL_MODE_SPLIT: u8 = 1;

pub fn color_count(level: u8, level_mode: u8) -> usize {
    if level_mode == LEVEL_MODE_SPLIT {
        4
    } else {
        // The original indexes past its 16-entry table for higher levels; the game never
        // gets there, so treat it as 5.
        COLORS_PER_LEVEL.get(level as usize).copied().unwrap_or(5)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeanOrders {
    pub p1: [BeanColor; ORDER_LEN],
    pub p2: [BeanColor; ORDER_LEN],
}

/// `GenPuyoOrder`. Consumes 256 RNG calls (504 in the split mode).
pub fn gen_bean_orders(rng: &mut Rng, level: u8, level_mode: u8) -> BeanOrders {
    gen_bean_orders_with(&mut || rng.next(), level, level_mode)
}

/// `GenPuyoOrder`, drawing from `random` (`Random`).
pub fn gen_bean_orders_with(random: &mut dyn FnMut() -> u16, level: u8, level_mode: u8) -> BeanOrders {
    let count = color_count(level, level_mode);
    let mut p1: [BeanColor; ORDER_LEN] = std::array::from_fn(|i| COLOR_CYCLE[i % count]);
    for i in (0..ORDER_LEN).rev() {
        let j = (random() & 0xFF) as usize;
        p1.swap(i, j);
    }

    let mut p2 = p1;
    if level_mode == LEVEL_MODE_SPLIT {
        // Player 2 keeps the first 8 beans; the other 248 get all 5 colors, reshuffled.
        let tail = &mut p2[8..];
        for (i, bean) in tail.iter_mut().enumerate() {
            *bean = COLOR_CYCLE[i % COLOR_CYCLE.len()];
        }
        for i in (0..tail.len()).rev() {
            let j = ((u32::from(random()) * tail.len() as u32) >> 16) as usize;
            tail.swap(i, j);
        }
    }
    BeanOrders { p1, p2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(order: &[BeanColor]) -> [usize; 7] {
        let mut c = [0; 7];
        for &b in order {
            c[b as usize] += 1;
        }
        c
    }

    #[test]
    fn early_levels_deal_four_colors_evenly() {
        let orders = gen_bean_orders(&mut Rng::new(99), 0, 0);
        assert_eq!(counts(&orders.p1), [64, 64, 0, 64, 64, 0, 0]);
        assert_eq!(orders.p1, orders.p2);
    }

    #[test]
    fn later_levels_deal_five_colors() {
        let orders = gen_bean_orders(&mut Rng::new(99), 3, 0);
        assert_eq!(counts(&orders.p1), [52, 51, 0, 51, 51, 51, 0]);
    }

    #[test]
    fn split_mode_shares_only_the_first_eight() {
        let mut rng = Rng::new(7);
        let orders = gen_bean_orders(&mut rng, 0, LEVEL_MODE_SPLIT);
        assert_eq!(orders.p1[..8], orders.p2[..8]);
        assert_eq!(counts(&orders.p2[8..]), [50, 50, 0, 50, 49, 49, 0]);
        assert_ne!(orders.p1, orders.p2);
    }

    #[test]
    fn consumes_256_calls() {
        let mut a = Rng::new(5);
        gen_bean_orders(&mut a, 4, 0);
        let mut b = Rng::new(5);
        (0..256).for_each(|_| {
            b.next();
        });
        assert_eq!(a, b);
    }
}
