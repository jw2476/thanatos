use std::{collections::HashMap, fmt::Display};

use rand::Rng;

use crate::equipment::EquipmentKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ItemKind {
    Wood,
    CopperOre,
    CopperIngot,
}

impl Display for ItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Wood => "Wood",
                Self::CopperOre => "Copper Ore",
                Self::CopperIngot => "Copper Ingot",
            }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

pub const RARITIES: [Rarity; 5] = [
    Rarity::Common,
    Rarity::Uncommon,
    Rarity::Rare,
    Rarity::Epic,
    Rarity::Legendary,
];

impl Rarity {
    pub fn next(&self) -> Self {
        match self {
            Self::Common => Self::Uncommon,
            Self::Uncommon => Self::Rare,
            Self::Rare => Self::Epic,
            Self::Epic => Self::Legendary,
            Self::Legendary => Self::Legendary,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub kind: ItemKind,
    pub rarity: Rarity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ItemStack {
    pub item: Item,
    pub quantity: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RecipeOutput {
    Items(ItemKind, usize),
    Equipment(EquipmentKind),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Recipe {
    pub inputs: Vec<(ItemKind, usize)>,
    pub output: RecipeOutput,
}

impl Recipe {
    pub fn craftable(&self, inventory: &[ItemStack], rarities: &[Rarity]) -> bool {
        self.inputs
            .iter()
            .zip(rarities)
            .all(|((kind, quantity), rarity)| {
                *quantity
                    <= inventory
                        .iter()
                        .find_map(|s| {
                            if s.item
                                == (Item {
                                    kind: *kind,
                                    rarity: *rarity,
                                })
                            {
                                Some(s.quantity)
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default()
            })
    }

    pub fn rarity_chances(&self, rarities: &[Rarity]) -> Vec<f32> {
        let total: f32 = self
            .inputs
            .iter()
            .map(|(_, quantity)| *quantity as f32)
            .sum();

        RARITIES
            .into_iter()
            .map(|query| {
                self.inputs
                    .iter()
                    .zip(rarities)
                    .map(|((_, quantity), rarity)| {
                        let mut output = 0.0;
                        if query == *rarity {
                            output += 0.8 * *quantity as f32
                        }
                        if query == rarity.next() {
                            output += 0.2 * *quantity as f32
                        }
                        output
                    })
                    .sum::<f32>()
                    / total
            })
            .collect()
    }
}

#[derive(Default, Debug)]
pub struct Inventory(HashMap<Item, usize>);

impl Inventory {
    pub fn add(&mut self, stack: ItemStack) {
        match self.0.get_mut(&stack.item) {
            Some(quantity) => *quantity += stack.quantity,
            None => {
                self.0.insert(stack.item, stack.quantity);
            }
        }
    }

    pub fn remove(&mut self, stack: ItemStack) -> Option<()> {
        self.0
            .get_mut(&stack.item)
            .map(|quantity| *quantity -= stack.quantity)
    }

    pub fn get(&self, item: Item) -> Option<usize> {
        self.0.get(&item).copied()
    }

    pub fn set(&mut self, stack: ItemStack) {
        match self.0.get_mut(&stack.item) {
            Some(quantity) => *quantity = stack.quantity,
            None => {
                self.0.insert(stack.item, stack.quantity);
            }
        }
    }

    pub fn items(&self) -> impl Iterator<Item = ItemStack> {
        self.0
            .clone()
            .into_iter()
            .map(|(item, quantity)| ItemStack { item, quantity })
    }
}

#[derive(Clone)]
pub struct LootTable<T> {
    entries: Vec<(f32, T)>,
}

impl<T> Default for LootTable<T> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
} 

impl<T> LootTable<T> {
    pub fn add(mut self, probability: f32, loot: T) -> Self {
        self.entries.push((probability, loot));
        self
    }

    pub fn pick(&self) -> &T {
        let mut rng = rand::thread_rng();
        let mut p: f32 = rng.gen();
        self.entries
            .iter()
            .find_map(|(probability, items)| {
                p -= probability;
                if p < 0.0 {
                    Some(items)
                } else {
                    None
                }
            })
            .unwrap()
    }
}