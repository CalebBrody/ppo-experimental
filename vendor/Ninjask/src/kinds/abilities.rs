//! Abilities with hand-ported behaviour: the `AbilityKind` registry.

use crate::dex::types::*;
use crate::state::{Terrain, Weather};

/// What an absorbing ability does when hit by its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbsorbEffect {
    /// Raise a stat by this many stages (Lightning Rod, Sap Sipper...).
    Boost(Boost, i8),
    /// Heal 1/4 of max HP (Volt Absorb, Water Absorb, Earth Eater).
    Heal,
    /// Flash Fire's boost to Fire moves.
    FlashFire,
}

/// Abilities with hand-ported behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbilityKind {
    /// No Ability.
    None,
    Intimidate,
    SetWeather(Weather),
    SetTerrain(Terrain),
    Levitate,
    // Stat stages.
    /// Defiant (Atk) and Competitive (SpA): +2 when a foe lowers a stat.
    StatDropReaction(Boost),
    Contrary,
    /// Clear Body, White Smoke, Full Metal Body: others can't lower its stats.
    ClearBody,
    HyperCutter,
    /// Inner Focus, Oblivious, Own Tempo, Scrappy: Intimidate fails (plus extras by id).
    IntimidateImmune,
    GuardDog,
    Rattled,
    FlowerVeil,
    // Reactions to damaging hits.
    /// Rough Skin, Iron Barbs: 1/8 of the attacker's HP on contact.
    ContactDamage,
    /// Flame Body, Static, Poison Point: 30% status on contact.
    ContactStatus(Status),
    PoisonTouch,
    /// Stamina, Weak Armor, Justified, Water Compaction, Steam Engine...: boosts when hit.
    HitBoost,
    ThermalExchange,
    SpicySpray,
    // Power and damage.
    Adaptability,
    /// Blaze, Torrent, Overgrow, Swarm: x1.5 at 1/3 HP or less.
    Pinch(TypeId),
    /// Aerilate, Pixilate, Refrigerate, Galvanize, Dragonize.
    TypeChange(TypeId),
    LiquidVoice,
    /// Tough Claws, Iron Fist, Strong Jaw, Mega Launcher, Sharpness.
    FlagBoost,
    Technician,
    /// Huge Power, Pure Power.
    HugePower,
    Hustle,
    Guts,
    FireMane,
    /// Fairy Aura, Dark Aura.
    Aura(TypeId),
    ThickFat,
    /// Multiscale, Shadow Shield.
    Multiscale,
    /// Filter, Solid Rock, Prism Armor.
    Filter,
    FriendGuard,
    Sniper,
    SuperLuck,
    /// Battle Armor, Shell Armor.
    CritImmune,
    CompoundEyes,
    NoGuard,
    /// Sends reflectable moves back at their user (TryHit, and TryHitSide
    /// for moves on its side).
    MagicBounce,
    /// Drops a move's secondary effects (and its `self` effects with them)
    /// for 1.3x power; Life Orb and Shell Bell don't act after it.
    SheerForce,
    Infiltrator,
    Sturdy,
    RockHead,
    // Immunities.
    GoodAsGold,
    /// Lightning Rod, Storm Drain, Volt/Water Absorb, Sap Sipper, Motor Drive,
    /// Earth Eater, Flash Fire, Well-Baked Body. `redirect`: draws single-target moves.
    Absorb {
        type_: TypeId,
        effect: AbsorbEffect,
        redirect: bool,
    },
    Soundproof,
    Bulletproof,
    Overcoat,
    Telepathy,
    /// Limber, Insomnia, Vital Spirit, Water Veil, Magma Armor, Immunity.
    StatusImmune(Status),
    /// Armor Tail, Queenly Majesty, Dazzling: block foes' priority moves.
    PriorityBlock,
    // Speed and priority.
    Prankster,
    GaleWings,
    /// Swift Swim, Chlorophyll, Sand Rush, Slush Rush: x2 Speed in their weather.
    WeatherSpeed(Weather),
    Unburden,
    SpeedBoost,
    /// 30% to Disable the move that hit it.
    CursedBody,
    /// Sets Toxic Spikes on the attacker's side when hit by a physical move.
    ToxicDebris,
    /// Copies a random adjacent foe's ability.
    Trace,
    /// Bounces stat drops from others back at their source.
    MirrorArmor,
    /// Half damage from contact moves.
    AuraGuard,
    /// Heals 1/16 at the end of each turn in rain.
    RainDish,
    /// x1.5 Special Attack in sun, and loses 1/8 each turn in it.
    SolarPower,
    /// Snow Cloak, Sand Veil: foes' accuracy x0.8 in their weather.
    WeatherEvasion(Weather),
    /// Prevents Roar and Dragon Tail from dragging the holder out.
    SuctionCups,
    /// Blocks explosion moves and Aftermath, none of which the engine
    /// supports yet: they must check for Damp when they are ported.
    Damp,
    /// Passes poison, paralysis and burn back to their source.
    Synchronize,
    // Switching.
    Hospitality,
    Unnerve,
    /// Adjacent foes can't switch out (unless Ghost-type or Shadow Tag).
    ShadowTag,
    Regenerator,
    NaturalCure,
    Pressure,
    /// Emergency Exit, Wimp Out: switch out when HP falls to half or less.
    EmergencyExit,
    /// Transforms into the foe across on switching in.
    Imposter,
    /// The holder's moves see sun (`Pokemon.effectiveWeather()`), and its
    /// attacks get sun's damage modifier.
    MegaSol,
    /// Air Lock, Cloud Nine: no weather effects while it is on the field.
    SuppressWeather,
    /// x2 Speed in Electric Terrain.
    SurgeSurfer,
    /// Grassy Terrain when hit.
    SeedSower,
    /// Sandstorm when hit.
    SandSpit,
    /// Heals 1/16 in snow.
    IceBody,
    /// Water heals 1/4 instead of hitting; Fire hits harder; rain heals and
    /// sun hurts 1/8 each turn.
    DrySkin,
    /// Cures its status at the end of a turn in rain.
    Hydration,
    /// No status (or Yawn) in sun.
    LeafGuard,
    /// Its side can't fall asleep (or get drowsy).
    SweetVeil,
    /// End of each turn: +2 to a random stat, -1 to another.
    Moody,
    /// End of each turn: may cure an adjacent ally's status.
    Healer,
    /// Heals a third of max HP after eating a berry.
    CheekPouch,
    /// Eats a berry again at the end of the next turn.
    CudChew,
    /// Clears the adjacent allies' stat changes on switching in.
    CuriousMedicine,
    /// Sleep ends twice as fast.
    EarlyBird,
    /// Castform takes the form of the weather.
    Forecast,
    /// Notes that it may eat pinch berries at half HP (none in this format).
    Gluttony,
    /// End of each turn: may bring back the berry it ate (always in sun).
    Harvest,
    /// End of each turn: may cure its own status.
    ShedSkin,
    /// Poison heals an eighth instead of hurting.
    PoisonHeal,
    /// End of each turn: takes an item an adjacent Pokémon used.
    Pickup,
    /// Berries work twice as hard.
    Ripen,
    /// Clears the screens on the field on switching in.
    ScreenCleaner,
    /// Water moves x2 from the holder, Fire moves x0.5 against it, no burn.
    WaterBubble,
    /// +1 Atk per Pokémon fainted when its move knocks one out.
    Moxie,
    /// Sound moves: x1.3 power from the holder, half damage to it.
    PunkRock,
    /// Rock, Ground and Steel moves x1.3 in sand.
    SandForce,
    /// Allies' (and its own) Steel moves x1.5.
    SteelySpirit,
    /// Defense x2.
    FurCoat,
    /// Defense x1.5 in Grassy Terrain.
    GrassPelt,
    /// Defense x1.5 with a status.
    MarvelScale,
    /// Halves Fire attacks against it and burn damage.
    Heatproof,
    /// Half damage from contact moves, double from Fire moves.
    Fluffy,
    /// x1.2 power for recoil and crash moves.
    Reckless,
    /// Double Atk and SpA against a target that just came in.
    Stakeout,
    /// Heavy Metal (x2), Light Metal (x0.5): the holder's weight.
    WeightChange,
    /// Ignores the other side's stat stages in damage and accuracy.
    Unaware,
    /// Multi-hit moves hit the most times, without accuracy checks.
    SkillLink,
    /// Damaging moves hit twice; the second hit does a quarter of the damage.
    ParentalBond,
    /// 30% to move first within its priority bracket with a damaging move.
    QuickDraw,
    /// The holder's moves lose their contact flag.
    LongReach,
    /// Supreme Overlord: x1.1 per fallen ally, up to 5.
    SupremeOverlord,
    /// Piercing Drill and Unseen Fist (Champions): contact moves go through
    /// Protect for a quarter of their damage.
    BypassProtect,
    /// Reveals the foes' held items on switch-in.
    Frisk,
    /// Gives its item to an ally that used up its own.
    Symbiosis,
    /// Steals the item of a contact attacker when it holds none.
    Pickpocket,
    /// Announces a foe's super effective or OHKO move on switch-in.
    Anticipation,
    /// 30% to infatuate an attacker that makes contact.
    CuteCharm,
    /// Announces a foe's strongest move on switch-in.
    Forewarn,
    /// The holder's item has no effect (`ignoringItem`).
    Klutz,
    /// Steals a hit target's item after its move, when it holds none.
    Magician,
    /// Terrain changes the holder's type (`onTerrainChange`).
    Mimicry,
    /// A contact attacker takes Mummy as its ability.
    Mummy,
    /// A contact attacker swaps abilities with the holder.
    WanderingSpirit,
    /// Takes on the ability of an ally that faints (`onAllyFaint`).
    Receiver,
    /// Can always switch out (Champions: `trapped` is cleared).
    RunAway,
    /// Moves a little later within its priority bracket (`fractionalPriority` -0.1).
    Stall,
    /// Its item can't be taken by another Pokémon's move or ability.
    StickyHold,
    /// Lowers each foe's evasion on switch-in, once per battle.
    SuperSweetSyrup,
    /// Doubles every stat stage change (`onChangeBoost`).
    Simple,
    /// Other Pokémon's breakable abilities are ignored during its moves.
    MoldBreaker,
    /// Mimikyu's disguise takes the first hit from a move.
    Disguise,
    /// Aegislash changes forme to attack or to use King's Shield.
    StanceChange,
    /// No damage but from moves.
    MagicGuard,
    /// Appears as the last healthy teammate until a damaging hit.
    Illusion,
    /// Protean, Libero: the user takes its move's type, once per switch-in.
    Protean,
    /// x1.3 power when moving after every other active Pokémon.
    Analytic,
    /// Palafin becomes Palafin-Hero on switching out.
    ZeroToHero,
    /// Maximum Attack after taking a critical hit.
    AngerPoint,
    /// +1 Special Attack on falling to half HP or less from a hit.
    Berserk,
    /// No status, no Yawn, half damage from Ghost moves.
    PurifyingSalt,
    /// Its moves keep their chosen target and can't be redirected.
    Stalwart,
    /// Gains the `charge` volatile when a move damages it.
    Electromorphosis,
    /// A contact move that faints it costs the attacker a quarter of its HP.
    Aftermath,
    /// It and its allies are protected from Attract, Disable, Encore,
    /// Heal Block, Taunt and Torment.
    AromaVeil,
    /// Its Defense can't be lowered by others.
    BigPecks,
    /// Keen Eye, Illuminate: its accuracy can't be lowered by others, and
    /// its moves ignore evasion.
    KeenEye,
    /// It can poison Steel and Poison types.
    Corrosion,
    /// Greninja-Bond's boost after a KO, Greninja-Ash's Water Shuriken: both
    /// need a forme the format never has, so it does nothing here.
    BattleBond,
    /// Levitate, and +best stat per Pokémon fainted by its moves.
    Eelevate,
    /// A contact hit may put the attacker to sleep, paralyse or poison it.
    EffectSpore,
    /// Plus, Minus: x1.5 Special Attack with an ally that has Plus or Minus.
    PlusMinus,
    /// Morpeko swaps formes at the end of each turn.
    HungerSwitch,
    /// Embody Aspect (all four): needs a Terastallized Ogerpon, which the
    /// format never has, so it does nothing here.
    EmbodyAspect,
    /// Gooey: a contact hit lowers the attacker's Speed.
    Gooey,
    /// Innards Out: its fainting hit costs the attacker the damage dealt.
    InnardsOut,
    /// Liquid Ooze: HP a foe drains from it hurts the drainer instead.
    LiquidOoze,
    /// Merciless: its moves always crit a poisoned target.
    Merciless,
    /// Opportunist: copies the stat boosts of its foes.
    Opportunist,
    /// Quick Feet: x1.5 Speed with a status, and paralysis doesn't halve it.
    QuickFeet,
    /// Rivalry: x1.25 power against the same gender, x0.75 against the other.
    Rivalry,
    /// Shield Dust: the secondary effects of moves hitting it are dropped
    /// (a secondary's effect on the user stays).
    ShieldDust,
    /// Steadfast: +1 Speed whenever a flinch stops its move.
    Steadfast,
    /// Stench: its damaging moves get a 10% flinch secondary.
    Stench,
    /// Tangled Feet: x0.5 accuracy against it while it is confused.
    TangledFeet,
    Unsupported,
}

impl AbilityKind {
    /// `type_id` resolves a type name in the format's type chart.
    pub fn from_id(id: &str, type_id: impl Fn(&str) -> Option<TypeId>) -> AbilityKind {
        let t = |name: &str| type_id(name).unwrap_or(crate::dex::types::NO_TYPE);
        use AbilityKind::*;
        match id {
            "noability" => None,
            "intimidate" => Intimidate,
            "drizzle" => SetWeather(Weather::Rain),
            "drought" => SetWeather(Weather::Sun),
            "sandstream" => SetWeather(Weather::Sand),
            "snowwarning" => SetWeather(Weather::Snow),
            "grassysurge" => SetTerrain(Terrain::Grassy),
            "psychicsurge" => SetTerrain(Terrain::Psychic),
            "electricsurge" => SetTerrain(Terrain::Electric),
            "mistysurge" => SetTerrain(Terrain::Misty),
            "levitate" => Levitate,
            "defiant" => StatDropReaction(Boost::Atk),
            "competitive" => StatDropReaction(Boost::Spa),
            "contrary" => Contrary,
            "clearbody" | "whitesmoke" | "fullmetalbody" => ClearBody,
            "hypercutter" => HyperCutter,
            "innerfocus" | "oblivious" | "owntempo" | "scrappy" => IntimidateImmune,
            "guarddog" => GuardDog,
            "rattled" => Rattled,
            "flowerveil" => FlowerVeil,
            "roughskin" | "ironbarbs" => ContactDamage,
            "flamebody" => ContactStatus(Status::Brn),
            "static" => ContactStatus(Status::Par),
            "poisonpoint" => ContactStatus(Status::Psn),
            "poisontouch" => PoisonTouch,
            "stamina" | "weakarmor" | "justified" | "watercompaction" | "steamengine" => HitBoost,
            "thermalexchange" => ThermalExchange,
            "spicyspray" => SpicySpray,
            "adaptability" => Adaptability,
            "blaze" => Pinch(t("Fire")),
            "torrent" => Pinch(t("Water")),
            "overgrow" => Pinch(t("Grass")),
            "swarm" => Pinch(t("Bug")),
            "aerilate" => TypeChange(t("Flying")),
            "pixilate" => TypeChange(t("Fairy")),
            "refrigerate" => TypeChange(t("Ice")),
            "galvanize" => TypeChange(t("Electric")),
            "dragonize" => TypeChange(t("Dragon")),
            "liquidvoice" => LiquidVoice,
            "toughclaws" | "ironfist" | "strongjaw" | "megalauncher" | "sharpness" => FlagBoost,
            "technician" => Technician,
            "hugepower" | "purepower" => HugePower,
            "hustle" => Hustle,
            "guts" => Guts,
            "firemane" => FireMane,
            "fairyaura" => Aura(t("Fairy")),
            "darkaura" => Aura(t("Dark")),
            "thickfat" => ThickFat,
            "multiscale" | "shadowshield" => Multiscale,
            "filter" | "solidrock" | "prismarmor" => Filter,
            "friendguard" => FriendGuard,
            "sniper" => Sniper,
            "superluck" => SuperLuck,
            "battlearmor" | "shellarmor" => CritImmune,
            "compoundeyes" => CompoundEyes,
            "noguard" => NoGuard,
            "magicbounce" => MagicBounce,
            "sheerforce" => SheerForce,
            "infiltrator" => Infiltrator,
            "sturdy" => Sturdy,
            "rockhead" => RockHead,
            "goodasgold" => GoodAsGold,
            "lightningrod" => Absorb {
                type_: t("Electric"),
                effect: AbsorbEffect::Boost(Boost::Spa, 1),
                redirect: true,
            },
            "stormdrain" => Absorb {
                type_: t("Water"),
                effect: AbsorbEffect::Boost(Boost::Spa, 1),
                redirect: true,
            },
            "voltabsorb" => Absorb {
                type_: t("Electric"),
                effect: AbsorbEffect::Heal,
                redirect: false,
            },
            "waterabsorb" => Absorb {
                type_: t("Water"),
                effect: AbsorbEffect::Heal,
                redirect: false,
            },
            "eartheater" => Absorb {
                type_: t("Ground"),
                effect: AbsorbEffect::Heal,
                redirect: false,
            },
            "sapsipper" => Absorb {
                type_: t("Grass"),
                effect: AbsorbEffect::Boost(Boost::Atk, 1),
                redirect: false,
            },
            "motordrive" => Absorb {
                type_: t("Electric"),
                effect: AbsorbEffect::Boost(Boost::Spe, 1),
                redirect: false,
            },
            "wellbakedbody" => Absorb {
                type_: t("Fire"),
                effect: AbsorbEffect::Boost(Boost::Def, 2),
                redirect: false,
            },
            "flashfire" => Absorb {
                type_: t("Fire"),
                effect: AbsorbEffect::FlashFire,
                redirect: false,
            },
            "soundproof" => Soundproof,
            "bulletproof" => Bulletproof,
            "overcoat" => Overcoat,
            "telepathy" => Telepathy,
            "limber" => StatusImmune(Status::Par),
            "insomnia" | "vitalspirit" => StatusImmune(Status::Slp),
            "waterveil" => StatusImmune(Status::Brn),
            "magmaarmor" => StatusImmune(Status::Frz),
            "immunity" => StatusImmune(Status::Psn),
            "armortail" | "queenlymajesty" | "dazzling" => PriorityBlock,
            "prankster" => Prankster,
            "galewings" => GaleWings,
            "swiftswim" => WeatherSpeed(Weather::Rain),
            "chlorophyll" => WeatherSpeed(Weather::Sun),
            "sandrush" => WeatherSpeed(Weather::Sand),
            "slushrush" => WeatherSpeed(Weather::Snow),
            "unburden" => Unburden,
            "speedboost" => SpeedBoost,
            "hospitality" => Hospitality,
            "unnerve" => Unnerve,
            "shadowtag" => ShadowTag,
            "regenerator" => Regenerator,
            "naturalcure" => NaturalCure,
            "pressure" => Pressure,
            "emergencyexit" | "wimpout" => EmergencyExit,
            "cursedbody" => CursedBody,
            "toxicdebris" => ToxicDebris,
            "trace" => Trace,
            "mirrorarmor" => MirrorArmor,
            "auraguard" => AuraGuard,
            "raindish" => RainDish,
            "solarpower" => SolarPower,
            "snowcloak" => WeatherEvasion(Weather::Snow),
            "sandveil" => WeatherEvasion(Weather::Sand),
            "suctioncups" => SuctionCups,
            "damp" => Damp,
            "synchronize" => Synchronize,
            "imposter" => Imposter,
            "megasol" => MegaSol,
            "airlock" | "cloudnine" => SuppressWeather,
            "surgesurfer" => SurgeSurfer,
            "seedsower" => SeedSower,
            "sandspit" => SandSpit,
            "icebody" => IceBody,
            "dryskin" => DrySkin,
            "hydration" => Hydration,
            "leafguard" => LeafGuard,
            "sweetveil" => SweetVeil,
            "moody" => Moody,
            "healer" => Healer,
            "cheekpouch" => CheekPouch,
            "cudchew" => CudChew,
            "curiousmedicine" => CuriousMedicine,
            "earlybird" => EarlyBird,
            "forecast" => Forecast,
            "gluttony" => Gluttony,
            "harvest" => Harvest,
            "shedskin" => ShedSkin,
            "poisonheal" => PoisonHeal,
            "pickup" => Pickup,
            "ripen" => Ripen,
            "screencleaner" => ScreenCleaner,
            "waterbubble" => WaterBubble,
            "moxie" => Moxie,
            "punkrock" => PunkRock,
            "sandforce" => SandForce,
            "steelyspirit" => SteelySpirit,
            "furcoat" => FurCoat,
            "grasspelt" => GrassPelt,
            "marvelscale" => MarvelScale,
            "heatproof" => Heatproof,
            "fluffy" => Fluffy,
            "reckless" => Reckless,
            "stakeout" => Stakeout,
            "heavymetal" | "lightmetal" => WeightChange,
            "unaware" => Unaware,
            "skilllink" => SkillLink,
            "parentalbond" => ParentalBond,
            "quickdraw" => QuickDraw,
            "longreach" => LongReach,
            "supremeoverlord" => SupremeOverlord,
            "piercingdrill" | "unseenfist" => BypassProtect,
            "frisk" => Frisk,
            "symbiosis" => Symbiosis,
            "pickpocket" => Pickpocket,
            "anticipation" => Anticipation,
            "cutecharm" => CuteCharm,
            "forewarn" => Forewarn,
            "klutz" => Klutz,
            "magician" => Magician,
            "mimicry" => Mimicry,
            "mummy" => Mummy,
            "wanderingspirit" => WanderingSpirit,
            "receiver" => Receiver,
            "runaway" => RunAway,
            "stall" => Stall,
            "stickyhold" => StickyHold,
            "supersweetsyrup" => SuperSweetSyrup,
            "simple" => Simple,
            "moldbreaker" => MoldBreaker,
            "disguise" => Disguise,
            "stancechange" => StanceChange,
            "magicguard" => MagicGuard,
            "illusion" => Illusion,
            "protean" | "libero" => Protean,
            "analytic" => Analytic,
            "zerotohero" => ZeroToHero,
            "angerpoint" => AngerPoint,
            "berserk" => Berserk,
            "purifyingsalt" => PurifyingSalt,
            "stalwart" => Stalwart,
            "electromorphosis" => Electromorphosis,
            "aftermath" => Aftermath,
            "aromaveil" => AromaVeil,
            "bigpecks" => BigPecks,
            "keeneye" | "illuminate" => KeenEye,
            "corrosion" => Corrosion,
            "battlebond" => BattleBond,
            "eelevate" => Eelevate,
            "effectspore" => EffectSpore,
            "plus" | "minus" => PlusMinus,
            "hungerswitch" => HungerSwitch,
            "embodyaspectteal"
            | "embodyaspectwellspring"
            | "embodyaspecthearthflame"
            | "embodyaspectcornerstone" => EmbodyAspect,
            "gooey" => Gooey,
            "innardsout" => InnardsOut,
            "liquidooze" => LiquidOoze,
            "merciless" => Merciless,
            "opportunist" => Opportunist,
            "quickfeet" => QuickFeet,
            "rivalry" => Rivalry,
            "shielddust" => ShieldDust,
            "steadfast" => Steadfast,
            "stench" => Stench,
            "tangledfeet" => TangledFeet,
            _ => Unsupported,
        }
    }
}
