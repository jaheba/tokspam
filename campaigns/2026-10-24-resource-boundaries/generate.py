"""Regenerate the parameterized sources retained by this resource campaign."""

import argparse
import json


def generate(name, parameters):
    count = parameters["count"]
    if not isinstance(count, int) or not 1 <= count <= 100_000:
        raise ValueError("count must be an integer from 1 to 100,000")

    if name == "late_call_keyword":
        return "f(" + "x," * count + "if)\n"
    if name == "late_duplicate_star_parameter":
        return "def f(" + ",".join(f"x{i}" for i in range(count)) + ",*,*):\n pass\n"
    if name == "late_dictionary_unpack":
        return "{" + ",".join(f"{i}:0" for i in range(count)) + ",**}\n"
    if name == "wide_slice_list":
        return "x=a[" + "0:1," * count + ":]\n"
    if name == "lambda_without_body":
        return "x=lambda " + ",".join(f"x{i}" for i in range(count)) + ":\n"
    if name == "flat_template_fields":
        return "x=t'" + "{x}" * count + "'\n"
    if name == "unique_nfkc_identifiers":
        return "".join(f"K{i}=0\n" for i in range(count))
    if name == "colliding_nfkc_identifiers":
        return "".join(f"K{i % 10}=0\n" for i in range(count))
    if name == "repeated_match_cases":
        return "match x:\n" + " case 0: pass\n" * count + " case if: pass\n"
    raise ValueError(f"unknown generator: {name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name")
    parser.add_argument("parameters", help="JSON object containing the generator parameters")
    args = parser.parse_args()
    print(generate(args.name, json.loads(args.parameters)), end="")


if __name__ == "__main__":
    main()
