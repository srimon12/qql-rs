#!/usr/bin/env python3
"""Deterministic corpus generation for the QQL-vs-Qdrant-SDK benchmark.

Two domains, mirroring the ingested collections they are modelled on:

  berlin.jsonl  — 8,000 short-term-rental listings (dense + bm25 sparse)
  legal.jsonl   — 2,000 legal precedents              (dense + colbert + bm25)

Everything is seeded (`--seed`, default 42): the same command on any machine
produces byte-identical corpora, so vectors embedded from them are identical
too. No network access required.

Output: ../data/{berlin,legal}.jsonl  (one JSON object per line)
"""
from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

DATA = Path(__file__).resolve().parent.parent / "data"

# ---------------------------------------------------------------- berlin ----
DISTRICTS = [
    "Mitte", "Kreuzberg", "Prenzlauer Berg", "Charlottenburg", "Neukölln",
    "Friedrichshain", "Schöneberg", "Wedding", "Moabit", "Wilmersdorf",
    "Spandau", "Lichtenberg",
]

AMENITIES = [
    "wifi", "kitchen", "washer", "dryer", "elevator", "heating", "balcony",
    "garden", "parking", "desk", "tv", "bathtub", "dishwasher", "crib",
    "smoking allowed", "pets allowed", "long term stays", "luggage dropoff",
    "coffee machine", "blackout curtains",
]

NAME_ADJ = [
    "Cozy", "Sunny", "Spacious", "Modern", "Charming", "Quiet", "Stylish",
    "Bright", "Renovated", "Central", "Peaceful", "Luminous", "Elegant",
    "Compact", "Family-friendly",
]

NAME_NOUN = [
    "apartment", "studio", "loft", "flat", "attic", "pied-à-terre",
    "townhouse", "maisonette", "guesthouse", "penthouse",
]

LANDMARKS = [
    "Brandenburg Gate", "Museum Island", "East Side Gallery", "Tempelhofer Feld",
    "Volkspark Friedrichshain", "Kreuzberg's Bergmannkiez", "Spree river",
    "Alexanderplatz", "Ku'damm", "Mauerpark", "Gendarmenmarkt", "Tiergarten",
]

DESC_TEMPLATES = [
    "This {adj} {noun} in {district} sits {walk} minutes from {landmark}. "
    "The space sleeps {guests} guests with a {bed} bed, {bath} bath and {amen3}. "
    "Guests highlight the {perk} in recent reviews.",

    "A {adj} {noun} on a quiet side street in {district}, {walk} minutes from "
    "{landmark}. Fits {guests} guests: {bed} bedroom, {bath} bath, and {amen3}. "
    "The {perk} makes longer stays easy.",

    "Renovated {noun} in the heart of {district}. Walk {walk} minutes to "
    "{landmark}. Sleeps {guests} with a {bed} bed and {bath} bath; {amen3} "
    "included. Reviewers mention the {perk} again and again.",
]

PERKS = [
    "natural light", "soundproofed windows", "fresh renovation", "green courtyard",
    "flexible check-in", "responsive host", "strong wifi", "comfortable mattresses",
    "well-equipped kitchen", "quiet street",
]

BEDS = ["king", "queen", "double", "two single", "sofa"]


def _amen3(r: random.Random) -> str:
    a = r.sample(AMENITIES, 3)
    return f"{a[0]}, {a[1]} and {a[2]}"


def gen_berlin(n: int, seed: int) -> list[dict]:
    r = random.Random(seed)
    out = []
    for i in range(n):
        district = r.choice(DISTRICTS)
        name = f"{r.choice(NAME_ADJ)} {r.choice(NAME_NOUN)} in {district}"
        desc = r.choice(DESC_TEMPLATES).format(
            adj=r.choice(NAME_ADJ).lower(), noun=r.choice(NAME_NOUN),
            district=district, walk=r.randint(2, 25), landmark=r.choice(LANDMARKS),
            guests=r.randint(1, 6), bed=r.choice(BEDS), bath=r.randint(1, 2),
            amen3=_amen3(r), perk=r.choice(PERKS),
        )
        out.append({
            "id": i + 1,
            "name": name,
            "description": desc,
            "district": district,
            "price": round(r.uniform(35, 260), 2),
            "rating": round(r.uniform(3.0, 5.0), 2),
            "guests": r.randint(1, 6),
            "amenities": r.sample(AMENITIES, r.randint(2, 6)),
        })
    return out


# ----------------------------------------------------------------- legal ----
COURTS = [
    "KG Berlin", "LG Berlin", "AG Mitte", "AG Charlottenburg", "OLG Koblenz",
    "BGH", "LG Hamburg", "OLG München", "VG Berlin", "BVerwG", "LG Frankfurt",
    "OLG Frankfurt", "LG Köln", "BFH",
]

LEGAL_TAGS = [
    "rent", "deposit", "eviction", "contract", "damages", "liability",
    "employment", "dismissal", "insurance", "neighbor", "noise", "renovation",
    "defect", "termination", "notice", "landlord", "tenant", "consumer",
    "data-protection", "competition", "copyright", "building", "purchase",
    "warranty", "family", "custody", "maintenance", "inheritance", "tax",
    "customs", "asylum", "residence", "traffic", "criminal", "procedure",
]

TITLE_TEMPLATES = [
    "{subject} in {context}: {holding}",
    "{holding} — {subject} ({context})",
    "On {subject} in {context}: the court {holding}",
]

SUBJECTS = [
    "rental deposit retention", "ordinary termination of a lease",
    "defect rent reduction", "unfair contract terms review",
    "extraordinary termination", "rent increase demand",
    "dismissal protection claim", "damage compensation claim",
    "seller's duty to cure", "neighbor noise injunction",
    "consumer revocation right", "liability for building defects",
    "maintenance obligation after separation", "compulsory portion claim",
    "data processing consent scope", "comparative advertising limits",
]

CONTEXTS = [
    "residential tenancy law", "commercial lease agreements",
    "employment relationships", "construction contracts",
    "consumer sales", "long-term accommodation",
    "property line disputes", "online marketplace sales",
    "software licensing", "insurance intermediaries",
]

HOLDINGS = [
    "the retention exceeded the lawful cap",
    "the notice period was miscalculated by the landlord",
    "the reduction threshold required a material defect",
    "the clause failed the transparency test",
    "the damage claim was partially unfounded",
    "the curing deadline was unreasonably short",
    "the consent did not cover secondary processing",
    "the comparative statement disparaged a competitor",
    "the duty to mitigate applied to the tenant",
    "the increase met the comparability standard",
]

SUMMARY_TEMPLATES = [
    "The {court} held that {holding_short} in a dispute over {subject}. "
    "The parties disputed {sub1} and {sub2}. The chamber reasoned that {reason}, "
    "citing established precedent on {area}. The claim was {outcome}. "
    "The ruling clarifies {area} for similar proceedings.",

    "In proceedings concerning {subject}, the {court} found {holding_short}. "
    "After reviewing {sub1} and {sub2}, the court concluded that {reason}. "
    "The decision {outcome} and is regularly cited in {area} matters.",

    "The {court} addressed {subject} and decided {holding_short}. "
    "Key issues included {sub1}; the panel examined {sub2} before holding {reason}. "
    "Outcome: the claim was {outcome}. Relevant to {area} practice.",
]

REASONS = [
    "statutory caps bind private agreements without exception",
    "form requirements protect both parties equally",
    "the burden of proof lay with the party asserting the exception",
    "proportionality limits any contractual penalty",
    "the duty of disclosure extends to known material risks",
    "a material defect requires objective verification",
    "consumer expectations set the transparency benchmark",
    "continued use after the deadline constitutes waiver",
]

SUB_ISSUES = [
    "the admissibility of the appeal", "the scope of the written form",
    "the calculation of offsets", "the allocation of court costs",
    "the weight of expert testimony", "the timing of the notice",
    "the standard of review on appeal", "the partial admissibility of evidence",
    "the interest rate applied", "the validity of the arbitration clause",
]

AREAS = [
    "tenancy law", "employment law", "construction law", "consumer law",
    "data protection law", "civil procedure", "family law", "commercial law",
]


def gen_legal(n: int, seed: int) -> list[dict]:
    r = random.Random(seed + 1)
    out = []
    for i in range(n):
        court = r.choice(COURTS)
        subject = r.choice(SUBJECTS)
        summary = r.choice(SUMMARY_TEMPLATES).format(
            court=court, holding_short=r.choice(HOLDINGS), subject=subject,
            sub1=r.choice(SUB_ISSUES), sub2=r.choice(SUB_ISSUES),
            reason=r.choice(REASONS), area=r.choice(AREAS),
            outcome=r.choice(["dismissed", "partially granted", "granted"]),
        )
        out.append({
            "id": i + 1,
            "case_ref": f"{r.choice(['VII', 'VIII', 'XII', 'V', 'IX'])} ZR {r.randint(10, 320)}/{r.randint(1990, 2025)}",
            "title": r.choice(TITLE_TEMPLATES).format(
                subject=subject.capitalize(), context=r.choice(CONTEXTS),
                holding=r.choice(HOLDINGS),
            ),
            "summary": summary,
            "court": court,
            "year": r.randint(1990, 2025),
            "cited_by": r.randint(0, 400),
            "tags": r.sample(LEGAL_TAGS, r.randint(2, 5)),
        })
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--berlin", type=int, default=8_000)
    ap.add_argument("--legal", type=int, default=2_000)
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()

    DATA.mkdir(parents=True, exist_ok=True)
    for name, rows, count in (("berlin", gen_berlin(args.berlin, args.seed), args.berlin),
                              ("legal", gen_legal(args.legal, args.seed), args.legal)):
        path = DATA / f"{name}.jsonl"
        with path.open("w") as f:
            for row in rows:
                f.write(json.dumps(row, ensure_ascii=False) + "\n")
        print(f"{path.relative_to(DATA.parent)}: {count} docs, {path.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
