//! Moves with hand-ported behaviour: the `MoveKind` registry and its helpers.
//!
//! Anything with special behaviour must be listed here to be supported; the
//! support check (dex/support.rs) treats unlisted moves, items and abilities
//! with Showdown code as unsupported.

use crate::state::{Terrain, Weather};

/// Protect and its variants, by the volatile each one sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProtectKind {
    /// Protect and Detect.
    Protect,
    SpikyShield,
    BanefulBunker,
    KingsShield,
    Obstruct,
}

impl ProtectKind {
    pub fn id(self) -> &'static str {
        match self {
            ProtectKind::Protect => "protect",
            ProtectKind::SpikyShield => "spikyshield",
            ProtectKind::BanefulBunker => "banefulbunker",
            ProtectKind::KingsShield => "kingsshield",
            ProtectKind::Obstruct => "obstruct",
        }
    }
    pub fn from_id(s: &str) -> Option<ProtectKind> {
        [
            ProtectKind::Protect,
            ProtectKind::SpikyShield,
            ProtectKind::BanefulBunker,
            ProtectKind::KingsShield,
            ProtectKind::Obstruct,
        ]
        .into_iter()
        .find(|k| k.id() == s)
    }
    /// The effect name in its `-singleturn` line.
    pub fn start_name(self) -> &'static str {
        match self {
            ProtectKind::SpikyShield | ProtectKind::BanefulBunker => "move: Protect",
            _ => "Protect",
        }
    }
    /// King's Shield and Obstruct let status moves through.
    pub fn blocks_status(self) -> bool {
        !matches!(self, ProtectKind::KingsShield | ProtectKind::Obstruct)
    }
}

/// Moves with hand-ported behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    /// Data only (or unsupported, see dex/support.rs).
    Plain,
    Protect(ProtectKind),
    FakeOut,
    SuckerPunch,
    Tailwind,
    TrickRoom,
    SetWeather(Weather),
    SetTerrain(Terrain),
    Reflect,
    LightScreen,
    AuroraVeil,
    WeatherBall,
    /// Typeless (`???`), with recoil of a quarter of the user's max HP.
    Struggle,
    /// +1 priority in Grassy Terrain for a grounded user.
    GrassyGlide,
    /// Secondary effect: a random status out of poison, paralysis, sleep.
    DireClaw,
    /// Low Kick, Grass Knot: power from the target's weight.
    WeightPower,
    /// Heavy Slam, Heat Crash: power from the weight ratio.
    WeightRatioPower,
    /// 50 + 50 per fainted Pokémon on the user's side.
    LastRespects,
    /// In Psychic Terrain (grounded user): x1.5 and hits both foes.
    ExpandingForce,
    /// Hurricane, Thunder: never miss in rain, 50% accuracy in sun.
    RainAccuracy,
    /// Blizzard: never misses in snow.
    SnowAccuracy,
    /// Eruption, Water Spout, Dragon Energy: power scales with the user's HP.
    HpPower,
    /// Psychic Fangs, Brick Break: break the target side's screens first.
    ScreenBreak,
    TerrainPulse,
    KnockOff,
    Acrobatics,
    /// First Impression: only on the first turn out (Champions: disabled after).
    FirstImpression,
    /// Super effective on Water types.
    FreezeDry,
    /// Super Fang: half the target's HP.
    HalfHp,
    WideGuard,
    QuickGuard,
    HelpingHand,
    FollowMe,
    RagePowder,
    ThroatChop,
    Taunt,
    /// Lowers Atk and SpA, then switches out if that worked.
    PartingShot,
    /// Charges on the first turn (`twoturnmove`) and strikes on the second.
    Charge(ChargeKind),
    /// Stomping Tantrum, Temper Flare: double power after a failed move.
    FailurePower,
    Encore,
    Imprison,
    Yawn,
    /// Heals half and drops the Flying type for the turn (via `self`).
    Roost,
    /// Can't miss its user, and doubles damage to it, until its next move.
    GlaiveRush,
    /// Breaks Protect, Wide Guard and Quick Guard.
    Feint,
    /// Raises every stat by 1 for a third of the user's max HP.
    ClangorousSoul,
    /// Trick, Switcheroo: swap held items with the target.
    Trick,
    /// Resets every active Pokémon's stat stages.
    Haze,
    /// Fails without a terrain; ends the terrain when it hits.
    SteelRoller,
    /// Makes the target pure Water type.
    Soak,
    /// Every active Pokémon faints after three more turns' ends.
    PerishSong,
    /// Swaps abilities with the target.
    SkillSwap,
    /// Ends the terrain after hitting (`onAfterHit`).
    IceSpinner,
    /// Three hits of 20, 40 and 60 power, each after the first with its own
    /// accuracy roll (`multiaccuracy`).
    TripleAxel,
    /// Entry hazard: Rock-type damage to each Pokémon switching in.
    StealthRock,
    /// Power from the target's remaining HP (up to 100).
    HardPress,
    /// 20 more power per positive stat stage of the user.
    PowerTrip,
    /// Sets Stealth Rock on the foe's side after hitting.
    StoneAxe,
    /// 50 more power per hit the user has taken since switching in (up to 350).
    RageFist,
    /// Fails unless the user is Electric-type; then it loses that type.
    DoubleShock,
    Substitute,
    /// Marks the user's slot (slot condition) and asks for a fainted
    /// teammate to bring back at half HP.
    RevivalBlessing,
    /// Becomes a copy of the target (`transformInto`).
    Transform,
    /// Bind, Wrap, Fire Spin, Infestation...: the `partiallytrapped` volatile.
    PartialTrap,
    /// Moonlight, Morning Sun, Synthesis: heal by the weather.
    WeatherHeal,
    Rest,
    Wish,
    HealingWish,
    /// The volatiles Leech Seed, Aqua Ring and Ingrain (from move data);
    /// Salt Cure's comes from its secondary.
    LeechSeed,
    AquaRing,
    Ingrain,
    SaltCure,
    /// The `gravity` pseudo-weather.
    Gravity,
    /// The `stockpile` volatile (up to 3 layers), and the moves that use it.
    Stockpile,
    SpitUp,
    Swallow,
    /// Side conditions: Safeguard on the user's side; Spikes, Toxic Spikes
    /// and Sticky Web on the foe's.
    Safeguard,
    Spikes,
    ToxicSpikes,
    StickyWeb,
    /// Sets Spikes on the foe's side after hitting.
    CeaselessEdge,
    /// Clears hazards, screens and terrain, lowering the target's evasion.
    Defog,
    /// Swaps the two sides' conditions.
    CourtChange,
    /// Sets snow, then the user switches out (`priorityChargeCallback`
    /// adds a one-turn volatile before the move).
    ChillyReception,
    /// Destroys the held item of every adjacent Pokémon.
    CorrosiveGas,
    /// Cures the status of the user's whole team.
    HealBell,
    /// The `magnetrise` volatile: five turns off the ground.
    MagnetRise,
    /// Hits adjacent foes, poisons them, frees the user of traps and hazards.
    MortalSpin,
    /// Rapid Spin: the same clearing, and its secondary raises the user's Speed.
    RapidSpin,
    /// Hits, and grounds the target (the `smackdown` volatile).
    SmackDown,
    /// Usable asleep (`sleepUsable`); fails when the user is awake.
    Snore,
    /// The `magicroom` pseudo-weather: held items stop working.
    MagicRoom,
    /// A delayed hit: the `futuremove` slot condition.
    FutureSight,
    /// The `wonderroom` pseudo-weather: Defense and Special Defense swap.
    WonderRoom,
    /// Every Pokémon holding a berry eats it.
    Teatime,
    /// Clears substitutes and hazards, then raises Attack and Speed.
    TidyUp,
    /// Hits for three turns (the `uproar` volatile locks the move in).
    Uproar,
    /// Steel Beam: loses half its max HP after the move, hit or miss.
    SteelBeam,
    /// High Jump Kick, Axe Kick, Supercell Slam: lose half the user's max HP
    /// when the move fails (`hasCrashDamage`).
    Crash,
    /// Alluring Voice: confuses a target that raised a stat this turn.
    AlluringVoice,
    /// Burning Jealousy: burns a target that raised a stat this turn.
    BurningJealousy,
    /// Fails unless the target holds an item, which it reveals.
    Poltergeist,
    /// Double power on a grounded target in Electric Terrain.
    RisingVoltage,
    /// Double power on a target hurt this turn.
    Assurance,
    /// Hex, Infernal Parade: double power on a target with a status.
    StatusPower,
    /// Barb Barrage, Venoshock: double power on a poisoned target.
    PoisonPower,
    /// Double power with a status but sleep, and no burn penalty.
    Facade,
    /// Double power after the user's stats were lowered this turn.
    LashOut,
    /// Secondary effect: a random status out of burn, paralysis, freeze.
    TriAttack,
    /// Breaks the target side's screens; its type follows Paldean Tauros.
    RagingBull,
    /// Power from the user's Speed over the target's.
    ElectroBall,
    /// Power from the target's Speed over the user's.
    GyroBall,
    /// Flail, Reversal: more power at lower HP.
    LowHpPower,
    /// Double power unless the target moves later this turn or just came in.
    Payback,
    /// +3 Atk when the move knocks out its target.
    FellStinger,
    /// 20 power for Ash-Greninja with Battle Bond.
    WaterShuriken,
    /// Ignores redirection (`tracksTarget`).
    SnipeShot,
    /// Fails unless the user has eaten a berry.
    Belch,
    /// Ten hits of 20 power, each after the first with its own accuracy roll.
    PopulationBomb,
    /// Explosion, Self-Destruct: the user faints before the move hits
    /// (`selfdestruct: 'always'`).
    Explode,
    /// Explodes like Explosion; x1.5 in Misty Terrain for a grounded user.
    MistyExplosion,
    /// Damage equal to the user's HP; the user faints (when it hits).
    FinalGambit,
    /// Beat Up: one hit per healthy team member, each with that member's Attack.
    BeatUp,
    /// Last Resort: fails until every other move of the user has been used.
    LastResort,
    /// Round: a queued Round goes next, with doubled power.
    Round,
    /// Avalanche: double power when the target hit the user this turn.
    Avalanche,
    /// Beak Blast: a charge that burns contact attackers (`priorityChargeCallback`).
    BeakBlast,
    /// Burn Up: needs a Fire-type user, which loses the type when it hits.
    BurnUp,
    /// Comeuppance: 1.5x the damage of the last attacker this turn, aimed at it.
    Comeuppance,
    /// Counter: double the physical damage taken this turn, back at its attacker.
    Counter,
    /// Eerie Spell: the target's last move loses 3 PP.
    EerieSpell,
    /// Endeavor: damage that brings the target down to the user's HP.
    Endeavor,
    /// Aura Wheel: Morpeko only; Electric, or Dark in Hangry mode; +1 Speed.
    AuraWheel,
    /// Fickle Beam: 3 in 10 to double its power.
    FickleBeam,
    /// Grav Apple: x1.5 power under Gravity.
    GravApple,
    /// Metal Burst: Comeuppance with Steel.
    MetalBurst,
    /// Mirror Coat: Counter for special hits.
    MirrorCoat,
    /// Fling: throws the held item, whose Fling power and effect it takes.
    Fling,
    /// Flying Press: Fighting with Flying's effectiveness added.
    FlyingPress,
    /// Focus Punch: a charge that fails if the user is hit first.
    FocusPunch,
    /// Shell Side Arm: the better of Physical and Special against the target.
    ShellSideArm,
    /// Sparkling Aria: cures a burn on its targets.
    SparklingAria,
    /// Syrup Bomb: a secondary that sticks the target, lowering its Speed.
    SyrupBomb,
    /// Upper Hand: fails unless the target is about to use a priority attack.
    UpperHand,
    /// Badly poisons; can't miss (nor be dodged) when a Poison type uses it.
    Toxic,
    /// Copies the target's stat stages and critical-hit volatiles.
    PsychUp,
    /// +2 critical-hit ratio (`focusenergy` volatile).
    FocusEnergy,
    /// An ally's critical-hit ratio +1, +2 for a Dragon type.
    DragonCheer,
    /// Half the user's max HP for +6 Attack.
    BellyDrum,
    /// +2 to a random stat that can still rise.
    Acupressure,
    /// Heals the target half its max HP (three quarters with Mega Launcher).
    HealPulse,
    /// +1 Atk and SpA, +2 each in sun.
    Growth,
    /// Disables the target's last move.
    Disable,
    /// Lowers the target's Atk and SpA by 2; the user faints.
    Memento,
    /// The user survives this turn's hits with at least 1 HP.
    Endure,
    /// Makes the target pure Psychic type.
    MagicPowder,
    /// +2 evasion and the `minimize` volatile.
    Minimize,
    /// Swaps places with the ally (a stall counter of its own).
    AllySwitch,
    /// Switches out, passing stat stages and most volatiles on.
    BatonPass,
    /// Bug Bite, Pluck: eat the target's berry.
    StealEat,
    /// The target moves next.
    AfterYou,
    /// The target moves last.
    Quash,
    /// Power Swap (Atk, SpA), Guard Swap (Def, SpD): trade stat stages.
    SwapBoosts,
    /// Trade Speed stats.
    SpeedSwap,
    /// Power Split (Atk, SpA), Guard Split (Def, SpD): average stats.
    Split,
    /// Inverts the target's stat stages.
    TopsyTurvy,
    /// Resets the target's stat stages.
    ClearSmog,
    /// Averages the user's and the target's HP.
    PainSplit,
    /// The user's `destinybond` volatile: a foe's move that knocks it out
    /// takes that foe down too.
    DestinyBond,
    /// Outrage, Thrash, Petal Dance: `lockedmove` for two or three turns,
    /// then fatigue confusion.
    LockedMove,
    /// Ghost user: half its HP to curse the target; otherwise -1 Spe, +1
    /// Atk and Def.
    Curse,
    /// Gives the target the user's ability.
    Entrainment,
    /// The user takes the target's ability.
    RolePlay,
    /// Gives the target Insomnia (and wakes it).
    WorrySeed,
    /// Infatuates a target of the opposite gender (`attract`).
    Attract,
    /// Block, Mean Look: the `trapped` volatile, linked to the user's `trapper`.
    Trap,
    /// Damage, then a secondary that traps the target while the user stays in.
    SpiritShackle,
    /// Damage, then the user and the target trap each other.
    JawLock,
    /// Thief, Covet: an item-less user takes the target's item after the hit.
    Thief,
    /// The target's move this turn becomes Electric (`electrify`).
    Electrify,
    /// Every active Pokémon is trapped until the end of next turn (a
    /// pseudo-weather).
    FairyLock,
    /// Charge: +1 SpD and the `charge` volatile (generic, `add_volatile`).
    ChargeUp,
    /// The target's ability is suppressed (`gastroacid`, generic `add_volatile`).
    GastroAcid,
    /// The user's next move can't miss the target (`lockon` on the user).
    LockOn,
    /// +1 Def and SpD to the user's side's Plus and Minus Pokémon.
    MagneticFlux,
    /// +1 to every stat; the user can't switch out any more (`noretreat`).
    NoRetreat,
    /// Traps the target and lowers its Defense and Sp. Def each turn.
    Octolock,
    /// Pollen Puff: damage to a foe, a heal of half max HP to an ally.
    PollenPuff,
    /// Swaps the user's stored Attack and Defense (`powertrick`).
    PowerTrick,
    /// Gives back the user's last used-up item (`lastItem`).
    Recycle,
    /// Torment: the target can't use the same move twice in a row (`torment`).
    Torment,
    /// The user takes on the target's types.
    ReflectType,
    /// Halves the user's HP, leaves a substitute for the Pokémon that comes
    /// in (`selfSwitch: 'shedtail'`).
    ShedTail,
    /// Takes 4 PP from the target's last move.
    Spite,
    /// Lowers the target's Attack and heals the user by it.
    StrengthSap,
    /// Raises Defense by 2 and eats the user's berry.
    StuffCheeks,
    /// Forest's Curse and Trick-or-Treat: add a third type (`addType`).
    AddType,
    /// The target's ability becomes Simple.
    SimpleBeam,
    /// Uses the battle's last move (`callsMove`).
    Copycat,
    /// Uses a random move of the sleeping user's (`callsMove`, `sleepUsable`).
    SleepTalk,
    /// The target uses its last move again at once.
    Instruct,
}

/// The two-turn moves that charge first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeKind {
    /// +1 SpA while charging; no charge turn in rain.
    ElectroShot,
    /// Solar Beam, Solar Blade: no charge turn in sun, half power in rain,
    /// sand and snow.
    Solar,
    /// +1 SpA while charging.
    MeteorBeam,
    SkyAttack,
    /// Phantom Force: the user vanishes while charging (its volatile lasts
    /// two turns and makes moves miss it).
    PhantomForce,
    /// Fly, Bounce, Dig, Dive: the user is out of reach while it charges,
    /// except for the few moves the move's volatile lets through.
    Fly,
    Bounce,
    Dig,
    Dive,
}

impl ChargeKind {
    /// The charge turn raises the user's Special Attack.
    pub fn spa_boost(self) -> bool {
        matches!(self, ChargeKind::ElectroShot | ChargeKind::MeteorBeam)
    }
    /// Moves can't hit the user while it charges (`onInvulnerability`).
    pub fn invulnerable(self) -> bool {
        matches!(
            self,
            ChargeKind::PhantomForce
                | ChargeKind::Fly
                | ChargeKind::Bounce
                | ChargeKind::Dig
                | ChargeKind::Dive
        )
    }
    /// `onInvulnerability` of the move's volatile: the moves it lets through
    /// (Phantom Force's is `false` for every move).
    pub fn lets_through(self, move_id: &str) -> bool {
        match self {
            ChargeKind::Fly | ChargeKind::Bounce => {
                matches!(
                    move_id,
                    "gust"
                        | "twister"
                        | "skyuppercut"
                        | "thunder"
                        | "hurricane"
                        | "smackdown"
                        | "thousandarrows"
                )
            }
            ChargeKind::Dig => matches!(move_id, "earthquake" | "magnitude"),
            ChargeKind::Dive => matches!(move_id, "surf" | "whirlpool"),
            _ => false,
        }
    }
    /// The moves that hit this charging Pokémon for double: Bounce's through
    /// `onSourceBasePower`, the others through `onSourceModifyDamage`.
    pub fn doubles(self, move_id: &str) -> bool {
        match self {
            ChargeKind::Fly | ChargeKind::Bounce => matches!(move_id, "gust" | "twister"),
            ChargeKind::Dig => matches!(move_id, "earthquake" | "magnitude"),
            ChargeKind::Dive => matches!(move_id, "surf" | "whirlpool"),
            _ => false,
        }
    }
    /// The duration of the charging move's own volatile (0: none).
    pub fn volatile_duration(self) -> u8 {
        if self.invulnerable() {
            2
        } else {
            0
        }
    }
    /// Weather in which the move strikes at once.
    pub fn instant_in(self) -> Option<Weather> {
        match self {
            ChargeKind::ElectroShot => Some(Weather::Rain),
            ChargeKind::Solar => Some(Weather::Sun),
            _ => None,
        }
    }
}

impl MoveKind {
    pub fn from_id(id: &str) -> MoveKind {
        match id {
            "protect" | "detect" => MoveKind::Protect(ProtectKind::Protect),
            "spikyshield" => MoveKind::Protect(ProtectKind::SpikyShield),
            "banefulbunker" => MoveKind::Protect(ProtectKind::BanefulBunker),
            "kingsshield" => MoveKind::Protect(ProtectKind::KingsShield),
            "obstruct" => MoveKind::Protect(ProtectKind::Obstruct),
            "wideguard" => MoveKind::WideGuard,
            "quickguard" => MoveKind::QuickGuard,
            "helpinghand" => MoveKind::HelpingHand,
            "followme" => MoveKind::FollowMe,
            "ragepowder" => MoveKind::RagePowder,
            "throatchop" => MoveKind::ThroatChop,
            "taunt" => MoveKind::Taunt,
            "partingshot" => MoveKind::PartingShot,
            "fakeout" => MoveKind::FakeOut,
            "suckerpunch" => MoveKind::SuckerPunch,
            "tailwind" => MoveKind::Tailwind,
            "trickroom" => MoveKind::TrickRoom,
            "reflect" => MoveKind::Reflect,
            "lightscreen" => MoveKind::LightScreen,
            "auroraveil" => MoveKind::AuroraVeil,
            "weatherball" => MoveKind::WeatherBall,
            "struggle" => MoveKind::Struggle,
            "grassyglide" => MoveKind::GrassyGlide,
            "direclaw" => MoveKind::DireClaw,
            "lowkick" | "grassknot" => MoveKind::WeightPower,
            "heavyslam" | "heatcrash" => MoveKind::WeightRatioPower,
            "lastrespects" => MoveKind::LastRespects,
            "expandingforce" => MoveKind::ExpandingForce,
            "hurricane" | "thunder" => MoveKind::RainAccuracy,
            "blizzard" => MoveKind::SnowAccuracy,
            "eruption" | "waterspout" | "dragonenergy" => MoveKind::HpPower,
            "psychicfangs" | "brickbreak" => MoveKind::ScreenBreak,
            "terrainpulse" => MoveKind::TerrainPulse,
            "knockoff" => MoveKind::KnockOff,
            "acrobatics" => MoveKind::Acrobatics,
            "firstimpression" => MoveKind::FirstImpression,
            "freezedry" => MoveKind::FreezeDry,
            "superfang" => MoveKind::HalfHp,
            "electroshot" => MoveKind::Charge(ChargeKind::ElectroShot),
            "solarbeam" | "solarblade" => MoveKind::Charge(ChargeKind::Solar),
            "meteorbeam" => MoveKind::Charge(ChargeKind::MeteorBeam),
            "skyattack" => MoveKind::Charge(ChargeKind::SkyAttack),
            "phantomforce" => MoveKind::Charge(ChargeKind::PhantomForce),
            "fly" => MoveKind::Charge(ChargeKind::Fly),
            "bounce" => MoveKind::Charge(ChargeKind::Bounce),
            "dig" => MoveKind::Charge(ChargeKind::Dig),
            "dive" => MoveKind::Charge(ChargeKind::Dive),
            "stompingtantrum" | "temperflare" => MoveKind::FailurePower,
            "encore" => MoveKind::Encore,
            "imprison" => MoveKind::Imprison,
            "yawn" => MoveKind::Yawn,
            "roost" => MoveKind::Roost,
            "glaiverush" => MoveKind::GlaiveRush,
            "feint" => MoveKind::Feint,
            "clangoroussoul" => MoveKind::ClangorousSoul,
            "trick" | "switcheroo" => MoveKind::Trick,
            "haze" => MoveKind::Haze,
            "steelroller" => MoveKind::SteelRoller,
            "soak" => MoveKind::Soak,
            "perishsong" => MoveKind::PerishSong,
            "skillswap" => MoveKind::SkillSwap,
            "icespinner" => MoveKind::IceSpinner,
            "tripleaxel" => MoveKind::TripleAxel,
            "stealthrock" => MoveKind::StealthRock,
            "hardpress" => MoveKind::HardPress,
            "powertrip" => MoveKind::PowerTrip,
            "stoneaxe" => MoveKind::StoneAxe,
            "ragefist" => MoveKind::RageFist,
            "doubleshock" => MoveKind::DoubleShock,
            "substitute" => MoveKind::Substitute,
            "revivalblessing" => MoveKind::RevivalBlessing,
            "transform" => MoveKind::Transform,
            "bind" | "wrap" | "firespin" | "whirlpool" | "sandtomb" | "infestation"
            | "snaptrap" => MoveKind::PartialTrap,
            "moonlight" | "morningsun" | "synthesis" => MoveKind::WeatherHeal,
            "rest" => MoveKind::Rest,
            "wish" => MoveKind::Wish,
            "healingwish" => MoveKind::HealingWish,
            "leechseed" => MoveKind::LeechSeed,
            "aquaring" => MoveKind::AquaRing,
            "ingrain" => MoveKind::Ingrain,
            "saltcure" => MoveKind::SaltCure,
            "gravity" => MoveKind::Gravity,
            "stockpile" => MoveKind::Stockpile,
            "spitup" => MoveKind::SpitUp,
            "swallow" => MoveKind::Swallow,
            "safeguard" => MoveKind::Safeguard,
            "spikes" => MoveKind::Spikes,
            "toxicspikes" => MoveKind::ToxicSpikes,
            "stickyweb" => MoveKind::StickyWeb,
            "ceaselessedge" => MoveKind::CeaselessEdge,
            "defog" => MoveKind::Defog,
            "courtchange" => MoveKind::CourtChange,
            "chillyreception" => MoveKind::ChillyReception,
            "corrosivegas" => MoveKind::CorrosiveGas,
            "healbell" => MoveKind::HealBell,
            "magnetrise" => MoveKind::MagnetRise,
            "mortalspin" => MoveKind::MortalSpin,
            "rapidspin" => MoveKind::RapidSpin,
            "smackdown" => MoveKind::SmackDown,
            "snore" => MoveKind::Snore,
            "magicroom" => MoveKind::MagicRoom,
            "futuresight" => MoveKind::FutureSight,
            "wonderroom" => MoveKind::WonderRoom,
            "teatime" => MoveKind::Teatime,
            "tidyup" => MoveKind::TidyUp,
            "uproar" => MoveKind::Uproar,
            "steelbeam" => MoveKind::SteelBeam,
            "highjumpkick" | "axekick" | "supercellslam" => MoveKind::Crash,
            "alluringvoice" => MoveKind::AlluringVoice,
            "burningjealousy" => MoveKind::BurningJealousy,
            "poltergeist" => MoveKind::Poltergeist,
            "risingvoltage" => MoveKind::RisingVoltage,
            "assurance" => MoveKind::Assurance,
            "hex" | "infernalparade" => MoveKind::StatusPower,
            "barbbarrage" | "venoshock" => MoveKind::PoisonPower,
            "facade" => MoveKind::Facade,
            "lashout" => MoveKind::LashOut,
            "storedpower" => MoveKind::PowerTrip,
            "triattack" => MoveKind::TriAttack,
            "ragingbull" => MoveKind::RagingBull,
            "electroball" => MoveKind::ElectroBall,
            "gyroball" => MoveKind::GyroBall,
            "flail" | "reversal" => MoveKind::LowHpPower,
            "payback" => MoveKind::Payback,
            "fellstinger" => MoveKind::FellStinger,
            "watershuriken" => MoveKind::WaterShuriken,
            "snipeshot" => MoveKind::SnipeShot,
            "belch" => MoveKind::Belch,
            "populationbomb" => MoveKind::PopulationBomb,
            "explosion" | "selfdestruct" => MoveKind::Explode,
            "mistyexplosion" => MoveKind::MistyExplosion,
            "finalgambit" => MoveKind::FinalGambit,
            "beatup" => MoveKind::BeatUp,
            "lastresort" => MoveKind::LastResort,
            "round" => MoveKind::Round,
            "avalanche" => MoveKind::Avalanche,
            "beakblast" => MoveKind::BeakBlast,
            "burnup" => MoveKind::BurnUp,
            "comeuppance" => MoveKind::Comeuppance,
            "counter" => MoveKind::Counter,
            "eeriespell" => MoveKind::EerieSpell,
            "endeavor" => MoveKind::Endeavor,
            "aurawheel" => MoveKind::AuraWheel,
            "ficklebeam" => MoveKind::FickleBeam,
            "gravapple" => MoveKind::GravApple,
            "metalburst" => MoveKind::MetalBurst,
            "mirrorcoat" => MoveKind::MirrorCoat,
            "fling" => MoveKind::Fling,
            "flyingpress" => MoveKind::FlyingPress,
            "focuspunch" => MoveKind::FocusPunch,
            "shellsidearm" => MoveKind::ShellSideArm,
            "sparklingaria" => MoveKind::SparklingAria,
            "syrupbomb" => MoveKind::SyrupBomb,
            "upperhand" => MoveKind::UpperHand,
            "toxic" => MoveKind::Toxic,
            "psychup" => MoveKind::PsychUp,
            "focusenergy" => MoveKind::FocusEnergy,
            "dragoncheer" => MoveKind::DragonCheer,
            "bellydrum" => MoveKind::BellyDrum,
            "acupressure" => MoveKind::Acupressure,
            "healpulse" => MoveKind::HealPulse,
            "growth" => MoveKind::Growth,
            "disable" => MoveKind::Disable,
            "memento" => MoveKind::Memento,
            "endure" => MoveKind::Endure,
            "magicpowder" => MoveKind::MagicPowder,
            "minimize" => MoveKind::Minimize,
            "allyswitch" => MoveKind::AllySwitch,
            "batonpass" => MoveKind::BatonPass,
            "bugbite" | "pluck" => MoveKind::StealEat,
            "afteryou" => MoveKind::AfterYou,
            "quash" => MoveKind::Quash,
            "powerswap" | "guardswap" => MoveKind::SwapBoosts,
            "speedswap" => MoveKind::SpeedSwap,
            "powersplit" | "guardsplit" => MoveKind::Split,
            "topsyturvy" => MoveKind::TopsyTurvy,
            "clearsmog" => MoveKind::ClearSmog,
            "painsplit" => MoveKind::PainSplit,
            "destinybond" => MoveKind::DestinyBond,
            "outrage" | "thrash" | "petaldance" => MoveKind::LockedMove,
            "curse" => MoveKind::Curse,
            "entrainment" => MoveKind::Entrainment,
            "roleplay" => MoveKind::RolePlay,
            "worryseed" => MoveKind::WorrySeed,
            "attract" => MoveKind::Attract,
            "block" | "meanlook" => MoveKind::Trap,
            "spiritshackle" => MoveKind::SpiritShackle,
            "jawlock" => MoveKind::JawLock,
            "thief" | "covet" => MoveKind::Thief,
            "electrify" => MoveKind::Electrify,
            "fairylock" => MoveKind::FairyLock,
            "charge" => MoveKind::ChargeUp,
            "gastroacid" => MoveKind::GastroAcid,
            "lockon" => MoveKind::LockOn,
            "magneticflux" => MoveKind::MagneticFlux,
            "noretreat" => MoveKind::NoRetreat,
            "octolock" => MoveKind::Octolock,
            "pollenpuff" => MoveKind::PollenPuff,
            "powertrick" => MoveKind::PowerTrick,
            "recycle" => MoveKind::Recycle,
            "torment" => MoveKind::Torment,
            "reflecttype" => MoveKind::ReflectType,
            "shedtail" => MoveKind::ShedTail,
            "spite" => MoveKind::Spite,
            "strengthsap" => MoveKind::StrengthSap,
            "stuffcheeks" => MoveKind::StuffCheeks,
            "forestscurse" | "trickortreat" => MoveKind::AddType,
            "simplebeam" => MoveKind::SimpleBeam,
            "copycat" => MoveKind::Copycat,
            "sleeptalk" => MoveKind::SleepTalk,
            "instruct" => MoveKind::Instruct,
            other => {
                if let Some(w) = Weather::from_id(other) {
                    MoveKind::SetWeather(w)
                } else if let Some(t) = Terrain::from_id(other) {
                    MoveKind::SetTerrain(t)
                } else {
                    MoveKind::Plain
                }
            }
        }
    }
}
