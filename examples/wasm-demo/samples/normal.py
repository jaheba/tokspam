"""A small, ordinary Python module for the screening demo."""

from collections import Counter


def word_counts(lines: list[str]) -> list[tuple[str, int]]:
    counts: Counter[str] = Counter()

    for line in lines:
        for word in line.lower().split():
            counts[word.strip(".,!?:;")] += 1

    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))


def summarize(lines: list[str]) -> str:
    entries = word_counts(lines)
    top = entries[:5]
    return ", ".join(f"{word}: {count}" for word, count in top)


if __name__ == "__main__":
    sample = [
        "Python source can be screened before parsing.",
        "Screening helps bound parser work on untrusted source.",
        "This ordinary module should stay within the limits.",
    ]
    print(summarize(sample))
