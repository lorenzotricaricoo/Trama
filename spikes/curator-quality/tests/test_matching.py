import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from curator_eval.matching import match_type, prf, text_score  # noqa: E402
from curator_eval.text import (approx_tokens, content_tokens, normalize, owner_match,  # noqa: E402
                               person_match_score)


class TextTests(unittest.TestCase):
    def test_normalize_accents_and_punctuation(self):
        self.assertEqual(normalize("L'identità è  già   OK!"), "l identita e gia ok")

    def test_content_tokens_stem_and_numbers(self):
        self.assertEqual(content_tokens("le fatture"), content_tokens("la fattura"))
        self.assertIn("2", content_tokens("due secondi"))
        self.assertEqual(set(content_tokens("2 secondi")), set(content_tokens("due secondi")))

    def test_approx_tokens_monotonic(self):
        self.assertEqual(approx_tokens(""), 0)
        self.assertLess(approx_tokens("ok"), approx_tokens("integrazione"))
        self.assertEqual(approx_tokens("a, b."), 4)


class PersonTests(unittest.TestCase):
    roster2 = ["Giulia Conti", "Giulia Ferri", "Chiara Villa"]

    def test_exact_and_alias(self):
        self.assertEqual(person_match_score("Elena Sartori", "Elena Sartori", [], []), 1.0)
        self.assertEqual(person_match_score("elena", "Elena Sartori", ["Elena"], []), 1.0)

    def test_first_name_unique_in_source(self):
        self.assertEqual(person_match_score("Chiara", "Chiara Villa", [], self.roster2), 0.8)

    def test_same_first_name_is_ambiguous(self):
        self.assertEqual(person_match_score("Giulia", "Giulia Conti", [], self.roster2), 0.0)

    def test_initial(self):
        roster = ["Marco Bellini", "Marco Ricci"]
        self.assertEqual(person_match_score("Marco B.", "Marco Bellini", [], roster), 0.8)
        self.assertEqual(person_match_score("Marco B.", "Marco Ricci", [], roster), 0.0)

    def test_more_specific_prediction(self):
        self.assertEqual(person_match_score("Marco Bellini", "Marco", [], ["Marco"]), 0.7)

    def test_owner(self):
        self.assertTrue(owner_match("Io", "Io", []))
        self.assertTrue(owner_match("l'utente", "Io", []))
        self.assertFalse(owner_match("Marco", "Io", ["Marco Bellini"]))
        self.assertTrue(owner_match(None, None, []))
        self.assertTrue(owner_match("Marco", "Marco Bellini", ["Marco Bellini", "Elena Sartori"]))
        self.assertFalse(owner_match("Giulia", "Giulia Ferri", ["Giulia Ferri", "Giulia Conti"]))


class ScoreTests(unittest.TestCase):
    truth = {"text": "Per il pilota login con email e magic link; l'SSO passa alla fase due",
             "keywords": ["magic link"],
             "evidence": "per il pilota login con email e magic link, l'SSO passa alla fase due"}

    def test_paraphrase_matches(self):
        pred = {"text": "Si userà il magic link via email al posto dell'SSO nel pilota",
                "evidence": "login con email e magic link"}
        self.assertGreaterEqual(text_score(pred, self.truth), 0.45)

    def test_unrelated_does_not_match(self):
        pred = {"text": "Pilota con venti clienti selezionati", "evidence": "confermiamo il pilota con venti clienti"}
        self.assertLess(text_score(pred, self.truth), 0.45)


class MatchTypeTests(unittest.TestCase):
    roster = ["Marco Bellini", "Elena Sartori"]
    truths = [
        {"text": "Preparare la stima dell'effort per il pilota", "keywords": ["stima"],
         "owner": "Marco Bellini", "status": "closed", "evidence": "Fatta, l'ho mandata giovedì."},
        {"text": "Inviare l'elenco dei clienti del pilota", "keywords": ["elenco", "clienti"],
         "owner": "Elena Sartori", "status": "closed", "evidence": "l'elenco dei clienti del pilota l'abbiamo ricevuto"},
        {"text": "Verificare la durata del link", "keywords": ["sicurezza"], "owner": "Elena Sartori",
         "status": "open", "optional": True, "evidence": "Devo verificare con la sicurezza"},
    ]

    def test_strict_vs_lenient(self):
        preds = [
            {"text": "Stima del pilota consegnata", "owner": "Marco", "status": "closed", "confidence": 0.9},
            {"text": "Mandare l'elenco clienti del pilota", "owner": "Elena", "status": "open", "confidence": 0.6},
            {"text": "Verificare con la sicurezza la scadenza", "owner": "Elena", "status": "open"},
            {"text": "Ordinare le pizze", "owner": None, "status": "open", "confidence": 0.3},
        ]
        r = match_type("actions", preds, self.truths, self.roster)
        self.assertEqual(r.tp_lenient, 2)
        self.assertEqual(r.tp, 1)            # second has the wrong status
        self.assertEqual(r.fp, 2)            # wrong status + pizza
        self.assertEqual(r.fn, 1)            # the wrong-status item is also a miss
        self.assertEqual(r.absorbed, 1)      # optional item absorbs its prediction
        self.assertEqual(r.fields["status"], [1, 2])
        self.assertEqual(r.fields["owner"], [2, 2])
        self.assertIn((0.9, True), r.conf)
        self.assertIn((0.6, False), r.conf)

    def test_one_to_one(self):
        preds = [{"text": "Preparare la stima", "owner": "Marco", "status": "closed"},
                 {"text": "Preparare la stima effort", "owner": "Marco", "status": "closed"}]
        r = match_type("actions", preds, self.truths[:1], self.roster)
        self.assertEqual((r.tp, r.fp, r.fn), (1, 1, 0))

    def test_judge_pairs_added(self):
        preds = [{"text": "Qualcosa di diverso", "owner": "Elena", "status": "closed"}]
        r = match_type("actions", preds, self.truths[1:2], self.roster, extra_pairs=[(0, 0)])
        self.assertEqual((r.tp, r.fp, r.fn), (1, 0, 0))

    def test_prf(self):
        self.assertEqual(prf(0, 0, 0), (None, None, None))
        p, r, f = prf(3, 1, 2)
        self.assertAlmostEqual(p, 0.75)
        self.assertAlmostEqual(r, 0.6)
        self.assertAlmostEqual(f, 2 * 0.75 * 0.6 / 1.35)
        self.assertEqual(prf(0, 2, 2)[2], 0.0)


if __name__ == "__main__":
    unittest.main()
