use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};

const WORDS_IN_PHRASE: usize = 4;

const EVEN_WORDS: [&str; 256] = [
    "aardvark", "absurd", "accrue", "acme", "adrift", "adult", "afflict", "ahead",
    "aimless", "algol", "allow", "alone", "ammo", "ancient", "apple", "artist",
    "assume", "athens", "atlas", "aztec", "baboon", "backfield", "backward", "banjo",
    "beaming", "bedlamp", "beehive", "beeswax", "befriend", "belfast", "berserk", "billiard",
    "bison", "blackjack", "blockade", "blowtorch", "bluebird", "bombast", "bookshelf", "brackish",
    "breadline", "breakup", "brickyard", "briefcase", "burbank", "button", "buzzard", "cement",
    "chairlift", "chatter", "checkup", "chisel", "choking", "chopper", "christmas", "clamshell",
    "classic", "classroom", "cleanup", "clockwork", "cobra", "commence", "concert", "cowbell",
    "crackdown", "cranky", "crowfoot", "crucial", "crumpled", "crusade", "cubic", "dashboard",
    "deadbolt", "deckhand", "dogsled", "dragnet", "drainage", "dreadful", "drifter", "dropper",
    "drumbeat", "drunken", "dupont", "dwelling", "eating", "edict", "egghead", "eightball",
    "endorse", "endow", "enlist", "erase", "escape", "exceed", "eyeglass", "eyetooth",
    "facial", "fallout", "flagpole", "flatfoot", "flytrap", "fracture", "framework", "freedom",
    "frighten", "gazelle", "geiger", "glitter", "glucose", "goggles", "goldfish", "gremlin",
    "guidance", "hamlet", "highchair", "hockey", "indoors", "indulge", "inverse", "involve",
    "island", "jawbone", "keyboard", "kickoff", "kiwi", "klaxon", "locale", "lockup",
    "merit", "minnow", "miser", "mohawk", "mural", "music", "necklace", "neptune",
    "newborn", "nightbird", "oakland", "obtuse", "offload", "optic", "orca", "payday",
    "peachy", "pheasant", "physique", "playhouse", "pluto", "preclude", "prefer", "preshrunk",
    "printer", "prowler", "pupil", "puppy", "python", "quadrant", "quiver", "quota",
    "ragtime", "ratchet", "rebirth", "reform", "regain", "reindeer", "rematch", "repay",
    "retouch", "revenge", "reward", "rhythm", "ribcage", "ringbolt", "robust", "rocker",
    "ruffled", "sailboat", "sawdust", "scallion", "scenic", "scorecard", "scotland", "seabird",
    "select", "sentence", "shadow", "shamrock", "showgirl", "skullcap", "skydive", "slingshot",
    "slowdown", "snapline", "snapshot", "snowcap", "snowslide", "solo", "southward", "soybean",
    "spaniel", "spearhead", "spellbind", "spheroid", "spigot", "spindle", "spyglass", "stagehand",
    "stagnate", "stairway", "standard", "stapler", "steamship", "sterling", "stockman", "stopwatch",
    "stormy", "sugar", "surmount", "suspense", "sweatband", "swelter", "tactics", "talon",
    "tapeworm", "tempest", "tiger", "tissue", "tonic", "topmost", "tracker", "transit",
    "trauma", "treadmill", "trojan", "trouble", "tumor", "tunnel", "tycoon", "uncut",
    "unearth", "unwind", "uproot", "upset", "upshot", "vapor", "village", "virus",
    "vulcan", "waffle", "wallet", "watchword", "wayside", "willow", "woodlark", "zulu",
];

const ODD_WORDS: [&str; 256] = [
    "adroitness", "adviser", "aftermath", "aggregate", "alkali", "almighty", "amulet", "amusement",
    "antenna", "applicant", "apollo", "armistice", "article", "asteroid", "atlantic", "atmosphere",
    "autopsy", "babylon", "backwater", "barbecue", "belowground", "bifocals", "bodyguard", "bookseller",
    "borderline", "bottomless", "bradbury", "bravado", "brazilian", "breakaway", "burlington", "businessman",
    "butterfat", "camelot", "candidate", "cannonball", "capricorn", "caravan", "caretaker", "celebrate",
    "cellulose", "certify", "chambermaid", "cherokee", "chicago", "clergyman", "coherence", "combustion",
    "commando", "company", "component", "concurrent", "confidence", "conformist", "congregate", "consensus",
    "consulting", "corporate", "corrosion", "councilman", "crossover", "crucifix", "cumbersome", "customer",
    "dakota", "decadence", "december", "decimal", "designing", "detector", "detergent", "determine",
    "dictator", "dinosaur", "direction", "disable", "disbelief", "disruptive", "distortion", "document",
    "embezzle", "enchanting", "enrollment", "enterprise", "equation", "equipment", "escapade", "eskimo",
    "everyday", "examine", "existence", "exodus", "fascinate", "filament", "finicky", "forever",
    "fortitude", "frequency", "gadgetry", "galveston", "getaway", "glossary", "gossamer", "graduate",
    "gravity", "guitarist", "hamburger", "hamilton", "handiwork", "hazardous", "headwaters", "hemisphere",
    "hesitate", "hideaway", "holiness", "hurricane", "hydraulic", "impartial", "impetus", "inception",
    "indigo", "inertia", "infancy", "inferno", "informant", "insincere", "insurgent", "integrate",
    "intention", "inventive", "istanbul", "jamaica", "jupiter", "leprosy", "letterhead", "liberty",
    "maritime", "matchmaker", "maverick", "medusa", "megaton", "microscope", "microwave", "midsummer",
    "millionaire", "miracle", "misnomer", "molasses", "molecule", "montana", "monument", "mosquito",
    "narrative", "nebula", "newsletter", "norwegian", "october", "ohio", "onlooker", "opulent",
    "orlando", "outfielder", "pacific", "pandemic", "pandora", "paperweight", "paragon", "paragraph",
    "paramount", "passenger", "pedigree", "pegasus", "penetrate", "perceptive", "performance", "pharmacy",
    "phonetic", "photograph", "pioneer", "pocketful", "politeness", "positive", "potato", "processor",
    "provincial", "proximate", "puberty", "publisher", "pyramid", "quantity", "racketeer", "rebellion",
    "recipe", "recover", "repellent", "replica", "reproduce", "resistor", "responsive", "retraction",
    "retrieval", "retrospect", "revenue", "revival", "revolver", "sandalwood", "sardonic", "saturday",
    "savagery", "scavenger", "sensation", "sociable", "souvenir", "specialist", "speculate", "stethoscope",
    "stupendous", "supportive", "surrender", "suspicious", "sympathy", "tambourine", "telephone", "therapist",
    "tobacco", "tolerance", "tomorrow", "torpedo", "tradition", "travesty", "trombonist", "truncated",
    "typewriter", "ultimate", "undaunted", "underfoot", "unicorn", "unify", "universe", "unravel",
    "upcoming", "vacancy", "vagabond", "vertigo", "virginia", "visitor", "vocalist", "voyager",
    "warranty", "waterloo", "whimsical", "wichita", "wilmington", "wyoming", "yesteryear", "yucatan",
];

pub fn verification_phrase(a: &VerifyingKey, b: &VerifyingKey) -> String {
    let (first, second) = if a.as_bytes() <= b.as_bytes() {
        (a.as_bytes(), b.as_bytes())
    } else {
        (b.as_bytes(), a.as_bytes())
    };

    let mut hasher = Sha256::new();
    hasher.update(first);
    hasher.update(second);
    let digest = hasher.finalize();

    digest
        .iter()
        .take(WORDS_IN_PHRASE)
        .enumerate()
        .map(|(position, byte)| {
            let index = *byte as usize;
            if position % 2 == 0 {
                EVEN_WORDS[index]
            } else {
                ODD_WORDS[index]
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn key() -> VerifyingKey {
        SigningKey::generate(&mut OsRng).verifying_key()
    }

    #[test]
    fn both_wordlists_are_256_unique_words() {
        for list in [&EVEN_WORDS, &ODD_WORDS] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), 256, "a PGP wordlist half must be 256 distinct words");
        }
    }

    #[test]
    fn the_two_wordlist_halves_do_not_overlap() {
        for even in EVEN_WORDS {
            assert!(
                !ODD_WORDS.contains(&even),
                "{even} appears in both halves — position-alternation can no longer catch a transposition"
            );
        }
    }

    #[test]
    fn phrase_alternates_between_the_two_wordlists() {
        let a = key();
        let b = key();
        let phrase = verification_phrase(&a, &b);
        let words: Vec<&str> = phrase.split('-').collect();
        assert_eq!(words.len(), WORDS_IN_PHRASE);
        assert!(EVEN_WORDS.contains(&words[0]));
        assert!(ODD_WORDS.contains(&words[1]));
        assert!(EVEN_WORDS.contains(&words[2]));
        assert!(ODD_WORDS.contains(&words[3]));
    }

    #[test]
    fn phrase_is_order_independent() {
        let a = key();
        let b = key();
        assert_eq!(verification_phrase(&a, &b), verification_phrase(&b, &a));
    }

    #[test]
    fn phrase_is_deterministic() {
        let a = key();
        let b = key();
        assert_eq!(verification_phrase(&a, &b), verification_phrase(&a, &b));
    }

    #[test]
    fn different_key_pairs_produce_different_phrases() {
        let a = key();
        let b = key();
        let c = key();
        assert_ne!(verification_phrase(&a, &b), verification_phrase(&a, &c));
    }

    #[test]
    fn phrase_has_expected_word_count() {
        let a = key();
        let b = key();
        let phrase = verification_phrase(&a, &b);
        assert_eq!(phrase.split('-').count(), WORDS_IN_PHRASE);
    }
}
