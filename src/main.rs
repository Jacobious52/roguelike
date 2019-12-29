rltk::add_wasm_support!();

use rltk::{console, Console, GameState, Point, Rltk};
use specs::prelude::*;
#[macro_use]
extern crate specs_derive;

mod components;
mod damage_system;
mod game_log;
mod gui;
mod inventory_system;
mod map;
mod map_indexing_system;
mod melee_combat_system;
mod monster_ai_system;
mod player;
mod rect;
mod spawner;
mod visibility_system;

use components::*;
use damage_system::DamageSystem;
use inventory_system::{ItemCollectionSystem, ItemDropSystem, ItemUseSystem};
use map::*;
use map_indexing_system::MapIndexingSystem;
use melee_combat_system::MeleeCombatSystem;
use monster_ai_system::MonsterAI;
use player::*;
use visibility_system::VisibilitySystem;

#[derive(PartialEq, Copy, Clone)]
pub enum RunState {
    AwaitingInput,
    PreRun,
    PlayerTurn,
    MonsterTurn,
    ShowInventory,
    ShowDropItem,
    ShowTargeting { range: i32, item: Entity },
}

pub struct State {
    ecs: World,
}

macro_rules! run_systems {
    ( $s:expr; $( $x:expr ),* ) => {
        {
            $(
                let mut sys = $x;
                sys.run_now(&$s);
            )*
        }
    };
}

impl State {
    fn run_systems(&mut self) {
        run_systems!(self.ecs;
            VisibilitySystem{},
            MonsterAI{},
            MapIndexingSystem{},
            MeleeCombatSystem{},
            DamageSystem{},
            ItemCollectionSystem{},
            ItemUseSystem{},
            ItemDropSystem{}
        );

        self.ecs.maintain();
    }
}

impl GameState for State {
    fn tick(&mut self, ctx: &mut Rltk) {
        ctx.cls();

        draw_map(&self.ecs, ctx);

        {
            let positions = self.ecs.read_storage::<Position>();
            let renderables = self.ecs.read_storage::<Renderable>();
            let map = self.ecs.fetch::<Map>();

            let mut data = (&positions, &renderables).join().collect::<Vec<_>>();
            data.sort_by(|&a, &b| b.1.render_order.cmp(&a.1.render_order));
            for (pos, render) in data.iter() {
                let idx = map.xy_idx(pos.x, pos.y);
                if map.visible_tiles[idx] {
                    ctx.set(pos.x, pos.y, render.fg, render.bg, render.glyph)
                }
            }

            gui::draw_ui(&self.ecs, ctx);
        }

        let mut new_run_state;
        {
            let run_state = self.ecs.fetch::<RunState>();
            new_run_state = *run_state;
        }

        match new_run_state {
            RunState::PreRun => {
                self.run_systems();
                new_run_state = RunState::AwaitingInput;
            }
            RunState::AwaitingInput => {
                new_run_state = player_input(self, ctx);
            }
            RunState::PlayerTurn => {
                self.run_systems();
                new_run_state = RunState::MonsterTurn;
            }
            RunState::MonsterTurn => {
                self.run_systems();
                new_run_state = RunState::AwaitingInput;
            }
            RunState::ShowInventory => {
                let result = gui::show_inventory(self, ctx);
                match result.0 {
                    gui::ItemMenuResult::Cancel => new_run_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                    gui::ItemMenuResult::Selected => {
                        let item_entity = result.1.unwrap();

                        let is_ranged = self.ecs.read_storage::<Ranged>();
                        let is_item_ranged = is_ranged.get(item_entity);

                        if let Some(is_item_ranged) = is_item_ranged {
                            new_run_state = RunState::ShowTargeting {
                                range: is_item_ranged.range,
                                item: item_entity,
                            };
                        } else {
                            let mut intent = self.ecs.write_storage::<WantsToUseItem>();
                            intent
                                .insert(
                                    *self.ecs.fetch::<Entity>(),
                                    WantsToUseItem {
                                        item: item_entity,
                                        target: None,
                                    },
                                )
                                .expect("Unable to insert intent");
                            new_run_state = RunState::PlayerTurn;
                        }
                    }
                }
            }
            RunState::ShowTargeting { range, item } => {
                let blast: i32;
                {
                    let aeo_items = self.ecs.read_storage::<AreaOfEffect>();
                    let aeo = aeo_items.get(item);
                    match aeo {
                        None => blast = 1,
                        Some(aeo) => blast = aeo.radius,
                    };
                }

                let result = gui::ranged_target(self, ctx, range, blast);
                match result.0 {
                    gui::ItemMenuResult::Cancel => new_run_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                    gui::ItemMenuResult::Selected => {
                        let mut intent = self.ecs.write_storage::<WantsToUseItem>();
                        intent
                            .insert(
                                *self.ecs.fetch::<Entity>(),
                                WantsToUseItem {
                                    item,
                                    target: result.1,
                                },
                            )
                            .expect("Unable to insert intent");
                        new_run_state = RunState::PlayerTurn;
                    }
                }
            }
            RunState::ShowDropItem => {
                let result = gui::drop_item_menu(self, ctx);
                match result.0 {
                    gui::ItemMenuResult::Cancel => new_run_state = RunState::AwaitingInput,
                    gui::ItemMenuResult::NoResponse => {}
                    gui::ItemMenuResult::Selected => {
                        let item_entity = result.1.unwrap();
                        let mut intent = self.ecs.write_storage::<WantsToDropItem>();
                        intent
                            .insert(
                                *self.ecs.fetch::<Entity>(),
                                WantsToDropItem { item: item_entity },
                            )
                            .expect("Unable to insert intent");
                        new_run_state = RunState::PlayerTurn;
                    }
                }
            }
        }

        {
            let mut run_writer = self.ecs.write_resource::<RunState>();
            *run_writer = new_run_state;
        }
        damage_system::delete_the_dead(&mut self.ecs);
    }
}

macro_rules! register_components {
    ( $s:expr; $( $x:path ),* ) => {
        {
            $(
                $s.register::<$x>();
            )*
        }
    };
}

fn main() {
    let mut context = Rltk::init_simple8x8(80, 50, "Hello Rust World", "resources");
    context.with_post_scanlines(true);
    let mut gs = State { ecs: World::new() };

    register_components!(gs.ecs;
        Position,
        Renderable,
        Player,
        Viewshed,
        Monster,
        Name,
        BlocksTile,
        CombatStats,
        WantsToMelee,
        SufferDamage,
        Item,
        WantsToPickupItem,
        InBackpack,
        WantsToUseItem,
        WantsToDropItem,
        Consumable,
        ProvidesHealing,
        Ranged,
        InflictsDamage,
        AreaOfEffect,
        Confusion
    );

    let map: Map = Map::new_map_rooms_and_corridors();
    let (player_x, player_y) = map.rooms[0].center();

    let player_entity = spawner::player(&mut gs.ecs, player_x, player_y);

    gs.ecs.insert(player_entity);
    gs.ecs.insert(RunState::PreRun);
    gs.ecs.insert(Point::new(player_x, player_y));
    gs.ecs.insert(game_log::GameLog {
        entries: vec!["Welcome to my game".to_string()],
    });
    gs.ecs.insert(rltk::RandomNumberGenerator::new());

    for room in map.rooms.iter().skip(1) {
        spawner::spawn_room(&mut gs.ecs, room);
    }

    gs.ecs.insert(map);

    rltk::main_loop(context, gs);
}
