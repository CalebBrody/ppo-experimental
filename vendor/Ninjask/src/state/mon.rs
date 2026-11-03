//! A Pokémon's battle state: stats, moves, volatiles and per-turn bookkeeping.

use std::sync::Arc;

use crate::dex::types::*;

/// Move "slots" that are not in the moveset, used by `SlotChoice::Move` and
/// `ActionKind::Move`: Struggle, and the recharge turn after Hyper Beam.
pub const STRUGGLE: usize = usize::MAX;
pub const RECHARGE: usize = usize::MAX - 1;
/// A locked move not in the moveset (a charge or `lockedmove` move Copycat
/// called): the action's `id` is the move.
pub const EXTERNAL: usize = usize::MAX - 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveSlot {
    pub id: MoveId,
    pub pp: u8,
    pub max_pp: u8,
    pub disabled: bool,
    /// Showdown `moveSlot.used`: set by `deductPP`, never reset (Last Resort).
    pub used: bool,
}

/// Volatile effects a Pokémon can carry. A volatile not represented here is
/// not simulated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Volatiles {
    pub flinch: bool,
    /// Turns of confusion left (0 = not confused).
    pub confusion: u8,
    /// Protected this turn, by which move.
    pub protect: Option<crate::kinds::ProtectKind>,
    /// Showdown's `stall` volatile: success denominator for the next protect
    /// (0 = no stall volatile) and its remaining duration.
    pub stall_counter: u16,
    pub stall_duration: u8,
    /// Showdown's `choicelock` volatile: the move a Choice item locks into.
    pub choice_lock: Option<MoveId>,
    /// Flash Fire's boost to Fire moves.
    pub flash_fire: bool,
    /// Unburden's Speed boost after losing the held item.
    pub unburden: bool,
    /// Helping Hands received this turn (each x1.5 power).
    pub helping_hand: u8,
    /// Follow Me / Rage Powder this turn.
    pub follow_me: bool,
    pub rage_powder: bool,
    /// Turns left of Throat Chop (no sound moves) and Taunt (no status moves).
    pub throat_chop: u8,
    pub taunt: u8,
    /// `mustrecharge` (Hyper Beam): its remaining duration (0 = absent).
    pub must_recharge: u8,
    /// `twoturnmove` (Electro Shot, Solar Beam): the charging move and the
    /// volatile's remaining duration.
    pub two_turn: Option<MoveId>,
    pub two_turn_duration: u8,
    /// The charging move's own volatile (`volatiles[move.id]`) and the
    /// target location it will strike.
    pub charging: Option<MoveId>,
    pub charging_target_loc: i8,
    /// Its duration when it has one (Phantom Force), 0 = none.
    pub charging_duration: u8,
    /// Encore: the move the Pokémon is locked into, and turns left.
    pub encore: Option<MoveId>,
    pub encore_duration: u8,
    /// Disable (from Cursed Body): the disabled move, and turns left.
    pub disable: Option<MoveId>,
    pub disable_duration: u8,
    /// Imprison: foes can't use the moves this Pokémon knows.
    pub imprison: bool,
    /// Yawn: turns left before falling asleep (0 = none).
    pub yawn: u8,
    /// Glaive Rush's drawback until the user's next move: moves can't miss
    /// it and deal double damage.
    pub glaive_rush: bool,
    /// Roost: not Flying-type this turn.
    pub roost: bool,
    /// Perish Song: residual turns left before fainting (`perishsong`
    /// duration), 0 = none.
    pub perish: u8,
    /// Heal Block (from Psychic Noise): turns left, 0 = none.
    pub heal_block: u8,
    /// Substitute: the substitute's HP, 0 = none.
    pub substitute: u16,
    /// `partiallytrapped` (Infestation, Fire Spin...): turns left (0 = none),
    /// the damage divisor (8, or 6 with Binding Band), the move, and its
    /// user's slot while the user stays on the field (`None` once it left).
    pub partial_trap: u8,
    pub partial_trap_divisor: u8,
    pub partial_trap_move: Option<MoveId>,
    pub partial_trap_source: Option<(usize, usize)>,
    /// Leech Seed, with its user's slot (`sourceSlot`: whoever stands there
    /// drinks).
    pub leech_seed: Option<(usize, usize)>,
    pub aqua_ring: bool,
    /// Ingrain: heals, traps, grounds and holds the Pokémon in place.
    pub ingrain: bool,
    pub salt_cure: bool,
    /// Stockpile: layers (0 = none), and how much each layer changed Defense
    /// and Special Defense (`effectState.def`/`spd`, counted down).
    pub stockpile: u8,
    pub stockpile_def: i8,
    pub stockpile_spd: i8,
    /// Chilly Reception's one-turn volatile (`priorityChargeCallback`).
    pub chilly_reception: bool,
    /// Magnet Rise: turns left, 0 = none.
    pub magnet_rise: u8,
    /// Smack Down's `smackdown`: grounded.
    pub smack_down: bool,
    /// Uproar: turns left, 0 = none.
    pub uproar: u8,
    /// `gem` (this turn): the holder's moves have x1.3 power.
    pub gem: bool,
    /// Counter's one-turn volatile: the last physical hit and who dealt it.
    pub counter: Option<CounterVol>,
    /// Mirror Coat's, the same for special hits.
    pub mirror_coat: Option<CounterVol>,
    /// Beak Blast's one-turn volatile (`priorityChargeCallback`).
    pub beak_blast: bool,
    /// Focus Punch's one-turn volatile, with its `lostFocus`.
    pub focus_punch: Option<bool>,
    /// Fling's volatile: the item goes at the next Update.
    pub fling: bool,
    /// The Metronome item's volatile.
    pub metronome: Option<MetronomeVol>,
    /// Sparkling Aria's volatile on a target (no state of its own).
    pub sparkling_aria: bool,
    /// Syrup Bomb's volatile: the Pokémon that threw it (`effectState.source`)
    /// and the turns left (`duration`, 0 = none).
    pub syrup_bomb: Option<(usize, usize)>,
    pub syrup_bomb_duration: u8,
    /// Focus Energy: +2 critical-hit ratio.
    pub focus_energy: bool,
    /// Dragon Cheer, with whether the Pokémon was Dragon-type when it
    /// started (`hasDragonType`: +2 rather than +1).
    pub dragon_cheer: Option<bool>,
    /// Endure this turn: a move's hit leaves at least 1 HP.
    pub endure: bool,
    /// Minimize: `minimize` moves can't miss and deal double damage.
    pub minimize: bool,
    /// Ally Switch's own stall counter (`allyswitch`, 0 = none) and duration.
    pub ally_switch_counter: u16,
    pub ally_switch_duration: u8,
    /// Speed Swap, Power Split or Guard Split changed the stored stats; the
    /// species' are restored when the volatiles clear (`setSpecies`).
    pub stats_changed: bool,
    /// Destiny Bond, until the Pokémon's next move.
    pub destiny_bond: bool,
    /// Curse (from a Ghost type): a quarter of max HP each turn.
    pub curse: bool,
    /// `lockedmove` (Outrage...): the move, its `duration` and `trueDuration`.
    pub locked_move: Option<MoveId>,
    pub locked_duration: u8,
    pub locked_true_duration: u8,
    /// `attract`: the Pokémon that infatuated this one, and whether it has
    /// left the field since (its `onUpdate` then ends the attraction).
    pub attract: Option<(usize, usize)>,
    pub attract_source_gone: bool,
    /// `trapped` (Block, Mean Look, Spirit Shackle, Jaw Lock): linked to
    /// this Pokémon's `trapper` volatile.
    pub trapped: Option<(usize, usize)>,
    /// `trapper`: the Pokémon this one keeps trapped, a bit per
    /// `(side, position)` (`side * 8 + position`); none when 0.
    pub trapper: u16,
    /// Charge: the next Electric move has double power.
    pub charge: bool,
    /// Electrify (this turn): the Pokémon's moves are Electric.
    pub electrify: bool,
    /// Gastro Acid: the ability is suppressed (`ignoringAbility`).
    pub gastro_acid: bool,
    /// Lock-On: turns left (`duration`, 0 = none), and the Pokémon it locked
    /// on to while that one stays on the field.
    pub lock_on_duration: u8,
    pub lock_on: Option<(usize, usize)>,
    /// No Retreat: the Pokémon is trapped.
    pub no_retreat: bool,
    /// Octolock: the Pokémon that holds this one (`effectState.source`).
    pub octolock: Option<(usize, usize)>,
    /// Power Trick: the stored Attack and Defense are swapped.
    pub power_trick: bool,
    /// Reflect Type on a foe: the user's apparent types (what the foe's
    /// side still sees) and its real ones at the time. Showdown's
    /// `apparentType` catches up with a silent `typechange` at the next
    /// turn's start, if the real types are still those.
    pub apparent_types: Option<([TypeId; 2], [TypeId; 2])>,
    /// Torment: the last move can't be chosen again.
    pub torment: bool,
    /// When each volatile with a residual handler was added (a battle-wide
    /// counter). Showdown keeps a Pokémon's volatiles in insertion order,
    /// and that order decides how ties shuffle in the residual sort.
    pub added: AddedStamps,
}

/// The volatiles with a residual handler, indexing `Volatiles::added`.
///
/// Do not reorder the variants: each discriminant is an index into
/// `Volatiles::added`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResidualVol {
    ThroatChop,
    Taunt,
    Encore,
    Disable,
    Yawn,
    Roost,
    Perish,
    HealBlock,
    Flinch,
    Protect,
    Stall,
    HelpingHand,
    FollowMe,
    RagePowder,
    MustRecharge,
    TwoTurnMove,
    Charging,
    PartialTrap,
    LeechSeed,
    AquaRing,
    Ingrain,
    SaltCure,
    ChillyReception,
    MagnetRise,
    Uproar,
    Gem,
    Counter,
    BeakBlast,
    MirrorCoat,
    FocusPunch,
    SyrupBomb,
    /// Not residual: Fling's place among the `Update` handlers.
    Fling,
    Endure,
    AllySwitch,
    Curse,
    LockedMove,
    /// Not residual: Charge's place among the `AfterMove` handlers.
    Charge,
    Electrify,
    LockOn,
    Octolock,
    /// Not residual: Attract's place among the `Update` handlers.
    Attract,
    /// Not a volatile: the count, so new variants above size `Volatiles::added`.
    Count,
}
pub const RESIDUAL_VOLS: usize = ResidualVol::Count as usize;

/// `Volatiles::added`: an array wrapper, since arrays longer than 32 have no
/// `Default`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AddedStamps(pub [u32; RESIDUAL_VOLS]);

impl Default for AddedStamps {
    fn default() -> Self {
        AddedStamps([0; RESIDUAL_VOLS])
    }
}

impl std::ops::Index<usize> for AddedStamps {
    type Output = u32;
    fn index(&self, i: usize) -> &u32 {
        &self.0[i]
    }
}

impl std::ops::IndexMut<usize> for AddedStamps {
    fn index_mut(&mut self, i: usize) -> &mut u32 {
        &mut self.0[i]
    }
}

/// Whether a Pokémon can't choose to switch out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Trapped {
    #[default]
    No,
    /// Trapped in secret (`tryTrap(true)`: Shadow Tag).
    Hidden,
    Yes,
}

/// Showdown's `abilityState` fields. Reset when the Pokémon switches in or
/// its ability changes; kept while it is benched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilityState {
    /// Trace still looking for an ability to copy (`seek`).
    pub seek: Option<bool>,
    /// Unnerve (`unnerved`): true from its start, false once it ends.
    pub unnerved: Option<bool>,
    /// Air Lock and Cloud Nine (`ending`): false from their start, true once
    /// they end; while false they suppress the weather.
    pub ending: Option<bool>,
    /// Gluttony (`gluttony`): set at its start and on any damage.
    pub gluttony: Option<bool>,
    /// Cud Chew: the berry to eat again, and the end-of-turn counter.
    pub cud_berry: Option<ItemId>,
    pub cud_counter: Option<i8>,
    /// Ripen (`berryWeaken`): it ate a resist berry, which weakens the hit again.
    pub berry_weaken: Option<bool>,
    /// Disguise took a hit (`busted`).
    pub busted: Option<bool>,
    /// Protean and Libero changed the type this switch-in (`protean`, `libero`).
    pub protean: Option<bool>,
    pub libero: Option<bool>,
    /// Berserk (`checkedBerserk`): false from a move's damage until its
    /// AfterMoveSecondary; healing berries wait while it is false.
    pub checked_berserk: Option<bool>,
    /// Opportunist (`boosts`): the foes' boosts it will copy, in the order
    /// Showdown first saw each stat (not projected: Showdown's projection
    /// drops objects).
    pub opp_boosts: Option<crate::dex::types::MoveBoosts>,
    /// Supreme Overlord (`fallen`): the fainted allies it counted at its start.
    pub fallen: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mon {
    // Identity and stats
    /// The Pokémon's name, which identifies it within its side. Shared, so
    /// the records that name a Pokémon (`Attacker`, a queued move's original
    /// target) copy it without allocating.
    pub name: Arc<str>,
    pub species: SpeciesId,
    /// Species before Mega Evolution (for requests and canMegaEvo).
    pub base_species: SpeciesId,
    /// Showdown `set.species`: the species the team listed (Beat Up reads its
    /// base Attack, even after a Mega Evolution).
    pub set_species: SpeciesId,
    pub types: [TypeId; 2],
    /// Showdown `addedType` (Forest's Curse, Trick-or-Treat), `NO_TYPE`
    /// without one. `setType` and `setSpecies` clear it.
    pub added_type: TypeId,
    pub level: u8,
    pub gender: String,
    /// Shown in its details (", shiny").
    pub shiny: bool,
    /// The set's nature as Showdown names it, or "" without one.
    pub nature_name: &'static str,
    pub nature_plus: Option<Stat>,
    pub nature_minus: Option<Stat>,
    pub points: [u16; 6],
    /// Showdown's storedStats; index 0 holds max HP.
    pub stats: [u16; 6],

    // HP, status and boosts
    pub hp: u16,
    pub maxhp: u16,
    pub status: Status,
    /// Sleep or freeze turns left (Showdown `statusState.time`).
    pub status_time: u8,
    /// Sleep or freeze turns at the start (`statusState.startTime`).
    pub status_start_time: u8,
    /// The status whose effect state `statusState` holds: the last one set,
    /// `None` after a cure. Fainting and leaving fainted keep it.
    pub status_state: Status,
    /// Toxic counter (Showdown `statusState.stage`).
    pub toxic_stage: u8,
    pub boosts: [i8; 7],
    /// Cached action speed (Showdown `pokemon.speed`).
    pub speed: i32,
    /// HP right after the last damage this turn that went through `spreadDamage`
    /// (Showdown `hurtThisTurn`); read by Assurance and Emergency Exit.
    pub hurt_this_turn: Option<u16>,

    // Item
    pub item: Option<ItemId>,
    /// The last item held before it was used, eaten or popped.
    pub last_item: Option<ItemId>,
    pub used_item_this_turn: bool,
    pub ate_berry: bool,
    /// Showdown `syrupTriggered` (Supersweet Syrup): kept for the battle.
    pub syrup_triggered: bool,

    // Ability
    pub ability: AbilityId,
    pub base_ability: AbilityId,
    pub ability_state: AbilityState,
    /// When `ability_state` was last created (the battle's stamp counter):
    /// Showdown's `abilityState.effectOrder`, which breaks speed ties in
    /// redirection. Not in the projection.
    pub ability_order: u32,
    /// Illusion: the name of the teammate this Pokémon appears as (Showdown
    /// `illusion`). Set at each switch-in; kept on the bench.
    pub illusion: Option<String>,
    /// Zero to Hero announced the Hero forme (Showdown `heroMessageDisplayed`).
    pub hero_message_displayed: bool,

    // Moves and volatiles
    pub moves: Vec<MoveSlot>,
    pub vol: Volatiles,
    pub last_move: Option<MoveId>,
    /// Target location of the last move (Showdown `lastMoveTargetLoc`).
    pub last_move_target_loc: i8,
    /// Showdown `moveThisTurnResult` / `moveLastTurnResult`: `Some(true)`
    /// the move did something, `Some(false)` it failed, `None` neither
    /// (no move, or a move blocked without failing, e.g. by Protect).
    pub move_this_turn_result: Option<bool>,
    pub move_last_turn_result: Option<bool>,
    /// Has already acted this turn (Showdown `moveThisTurn` truthiness).
    pub moved_this_turn: bool,
    /// Moves used since switching in (Showdown `activeMoveActions`).
    pub active_move_actions: u16,
    /// Hits taken from moves since switching in (Showdown `timesAttacked`;
    /// Champions resets it in `clearVolatile`), for Rage Fist.
    pub times_attacked: u16,
    /// Showdown `attackedBy`, numbered damage only (Avalanche, Comeuppance).
    pub attacked_by: Vec<Attacker>,
    /// Showdown `statsRaisedThisTurn`: a boost raised a stat this turn.
    /// Alluring Voice, Burning Jealousy.
    pub stats_raised_this_turn: bool,
    /// Showdown `statsLoweredThisTurn`: a boost lowered a stat this turn. Lash Out.
    pub stats_lowered_this_turn: bool,

    // Position and switching
    /// Index in the side's current Pokémon order (Showdown `position`).
    pub position: usize,
    pub is_active: bool,
    /// Revived by Revival Blessing in the slot where it fainted, until it
    /// switches back in: on the field but not active (Showdown `isActive`).
    pub revived: bool,
    pub fainted: bool,
    pub faint_queued: bool,
    pub switch_flag: bool,
    /// Showdown `forceSwitchFlag`: dragged out by Roar after the action.
    pub force_switch_flag: bool,
    /// The switch flag was set by the Pokémon's own move (U-turn), not forced
    /// (Showdown's `switchFlag` holds the move id rather than `true`).
    /// The move whose self-switch set `switch_flag` (U-turn...), named in the switch line.
    pub switch_from_move: Option<MoveId>,
    /// BeforeSwitchOut already ran when the switch request was made, so the
    /// switch skips it (Showdown `skipBeforeSwitchOutEventFlag`).
    pub skip_before_switch_out: bool,
    pub mega_evolved: bool,
    /// Showdown `trapped` from the end-of-turn trap pass: can't choose to
    /// switch. `Hidden` (Shadow Tag) isn't shown in the request of the
    /// side's last active Pokémon.
    pub trapped: Trapped,
    /// Showdown `maybeTrapped`: the request warns it may be trapped.
    pub maybe_trapped: bool,
    pub active_turns: u16,

    // Transform
    /// Transform (Showdown `transformed`): the Pokémon's own move slots and
    /// stats, restored when it leaves the field.
    pub transformed: Option<Box<crate::transform::Transformed>>,
}

/// Counter's one-turn volatile (`effectState.slot` and `.damage`): where the
/// last physical hit came from, as (side, position), and double its damage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CounterVol {
    pub slot: Option<(u8, u8)>,
    pub damage: u32,
}

/// The Metronome item's volatile (`lastMove` and `numConsecutive`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MetronomeVol {
    pub last_move: Option<MoveId>,
    pub num: u16,
}

/// One entry of Showdown's `attackedBy` for a hit that dealt a damage number
/// (`damageValue` a number): who hit, how hard (the last hit's damage), and
/// from which slot. The source is named by its side and name, which outlast
/// the position swaps of switching.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Attacker {
    pub side: u8,
    pub name: Arc<str>,
    pub damage: u32,
    pub mv: crate::dex::types::MoveId,
    pub this_turn: bool,
    pub slot: (u8, u8),
}

impl Mon {
    pub fn has_type(&self, t: TypeId) -> bool {
        self.types[0] == t || self.types[1] == t
    }
}
