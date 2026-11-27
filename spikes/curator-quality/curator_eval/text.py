"""Text normalisation, approximate tokenizer and similarity used by the matcher.

Everything here is deterministic and documented in the report:

- ``normalize``: Unicode NFKD, accents removed, lower case, apostrophes and
  punctuation turned into spaces, whitespace collapsed.
- ``content_tokens``: normalised words minus Italian/English stop words, number
  words mapped to digits ("due" -> "2"), then a crude stem: the first 5 characters
  of each word ("fatture" and "fattura" -> "fattu"). Crude, but language-agnostic
  and good enough for Italian inflection.
- ``dice``: 2|A∩B| / (|A|+|B|) on token sets.
- ``overlap``: |A∩B| / min(|A|,|B|) on token sets.
"""

from __future__ import annotations

import math
import re
import unicodedata

STOP_WORDS = frozenset(
    """
    a ad al alla alle allo agli ai anche ancora che chi ci col come con cosa cui da dal dalla dalle
    dallo dagli dai de dei del della delle dello degli di e ed era essere fa fra gli ha hanno ho i il
    in io la le lei li lo loro lui ma mi ne nei nel nella nelle nello negli noi non o per perche piu
    poi quale quando quella quelle quello questa queste questo quindi se sei si sia siamo sono su sua
    sue sui sul sulla sulle suo suoi ti tra tu tutti tutto un una uno vi voi gia deve devono essere
    va vanno viene e stato stata sara solo ogni dopo prima entro
    the of to and or for in on at by with from is are be was were will must should can an as that
    this these those it its we you they our your their not no
    """.split()
)

NUMBER_WORDS = {
    "uno": "1", "una": "1", "due": "2", "tre": "3", "quattro": "4", "cinque": "5", "sei": "6",
    "sette": "7", "otto": "8", "nove": "9", "dieci": "10", "quindici": "15", "venti": "20",
    "ventidue": "22", "trenta": "30", "quaranta": "40", "cinquanta": "50", "sessanta": "60",
    "settanta": "70", "ottanta": "80", "novanta": "90", "cento": "100", "primo": "1",
    "one": "1", "two": "2", "three": "3", "five": "5", "ten": "10", "twenty": "20",
}
# "una"/"sei"/"primo" are also ordinary words; mapping them to digits only matters for
# matching and is applied to both sides, so it is harmless.

STEM_LEN = 5


def normalize(text: str | None) -> str:
    if not text:
        return ""
    text = unicodedata.normalize("NFKD", str(text))
    text = "".join(c for c in text if not unicodedata.combining(c))
    text = text.lower()
    text = re.sub(r"[^\w]+", " ", text)
    text = text.replace("_", " ")
    return re.sub(r"\s+", " ", text).strip()


def stem(word: str) -> str:
    return word[:STEM_LEN]


def content_tokens(text: str | None) -> list[str]:
    out = []
    for w in normalize(text).split():
        if w in STOP_WORDS and w not in NUMBER_WORDS:
            continue
        w = NUMBER_WORDS.get(w, w)
        out.append(stem(w))
    return out


def all_tokens(text: str | None) -> list[str]:
    """Normalised, stemmed tokens *without* stop-word removal (used for roles: 'IT')."""
    return [stem(NUMBER_WORDS.get(w, w)) for w in normalize(text).split()]


def dice(a: set, b: set) -> float:
    if not a or not b:
        return 0.0
    return 2 * len(a & b) / (len(a) + len(b))


def overlap(a: set, b: set) -> float:
    if not a or not b:
        return 0.0
    return len(a & b) / min(len(a), len(b))


# --------------------------------------------------------------------------- tokens

_PRETOKEN = re.compile(r"\w+|[^\w\s]", re.UNICODE)


def approx_tokens(text: str) -> int:
    """Approximate BPE token count (documented heuristic, about +-15% vs o200k-style
    tokenizers on Italian/English prose; slightly conservative on JSON).

    Each word costs 1 token plus 1 per further 4 characters beyond the first 4
    (so 'ok' = 1, 'portale' = 1, 'integrazione' = 3); each punctuation mark costs 1.
    """
    n = 0
    for piece in _PRETOKEN.findall(text or ""):
        if piece[0].isalnum() or piece[0] == "_":
            n += 1 + max(0, math.ceil((len(piece) - 4) / 4))
        else:
            n += 1
    return n


# --------------------------------------------------------------------------- names

USER_ALIASES = frozenset({"io", "me", "utente", "l utente", "user", "the user", "io utente"})
NOBODY = frozenset({"", "nessuno", "non assegnato", "none", "null", "n a", "na", "unassigned", "tbd"})


def name_tokens(name: str | None) -> list[str]:
    return [t for t in normalize(name).split() if t not in {"dott", "dr", "ing", "sig", "sig ra"}]


def _tok_match(p: str, t: str) -> bool:
    """Token equality, with initials: 'b' (from 'Marco B.') matches 'bellini'."""
    if p == t:
        return True
    return len(p) == 1 and t.startswith(p)


def name_covers(pred: list[str], target: list[str]) -> bool:
    """True if every predicted token matches a distinct target token."""
    if not pred:
        return False
    remaining = list(target)
    for p in pred:
        for i, t in enumerate(remaining):
            if _tok_match(p, t):
                del remaining[i]
                break
        else:
            return False
    return True


def person_match_score(pred_name: str | None, truth_name: str, aliases: list[str],
                       roster: list[str]) -> float:
    """Score a predicted person name against a ground-truth person.

    1.0  exact match with the name or an alias;
    0.8  the predicted name is a part of the true name ('Marco B.', 'Elena') and no
         other person of the same source also fits it (so 'Giulia' does not match when
         two Giulias are in the source);
    0.7  the true name is a part of the predicted name (prediction is more specific);
    0.0  otherwise.
    """
    p = name_tokens(pred_name)
    if not p:
        return 0.0
    cands = [name_tokens(truth_name)] + [name_tokens(a) for a in aliases or []]
    if any(p == c for c in cands):
        return 1.0
    full = name_tokens(truth_name)
    if name_covers(p, full):
        fitting = [r for r in roster if name_covers(p, name_tokens(r))]
        if len(fitting) <= 1:
            return 0.8
    if full and name_covers(full, p):
        return 0.7
    return 0.0


def owner_match(pred_owner: str | None, truth_owner: str | None, roster: list[str]) -> bool:
    n = normalize(pred_owner)
    if truth_owner is None:
        return n in NOBODY
    if normalize(truth_owner) == "io":
        return n in USER_ALIASES or n.startswith("io ")
    if n in NOBODY or n in USER_ALIASES:
        return False
    return person_match_score(pred_owner, truth_owner, [], roster) > 0
