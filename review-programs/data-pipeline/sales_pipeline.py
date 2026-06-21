# =============================================================================
# sales_pipeline.py  --  CSV sales-analytics pipeline in pyrst
# =============================================================================
# A self-contained, deterministic data-analysis program:
#
#   1. Parse CSV-like records held in an in-program string (embedded "file").
#   2. Validate/clean rows, skipping malformed records (error handling).
#   3. Build typed Record objects.
#   4. Compute global aggregates: count / sum / avg / min / max.
#   5. Group-by region and by category, producing per-group aggregates.
#   6. Filter + transform (high-value orders, discount projections).
#   7. Rank groups and print a formatted, column-aligned report.
#
# Heavy use of: classes + methods + dunder, list/dict/set comprehensions,
# string methods, dict group-by (reassign-whole-value to dodge the nested
# subscript-mutation caveat), lambda-key sorting, ternaries, try/except,
# and f-string format specs for column alignment.
# =============================================================================


# -----------------------------------------------------------------------------
# Domain model
# -----------------------------------------------------------------------------

class Record:
    order_id: int
    region: str
    category: str
    units: int
    unit_price: float
    discount: float          # fractional, 0.0 .. 1.0

    def __init__(self, order_id: int, region: str, category: str,
                 units: int, unit_price: float, discount: float) -> None:
        self.order_id = order_id
        self.region = region
        self.category = category
        self.units = units
        self.unit_price = unit_price
        self.discount = discount

    def gross(self) -> float:
        return self.units * self.unit_price

    def net(self) -> float:
        return self.gross() * (1.0 - self.discount)

    def is_high_value(self) -> bool:
        return self.net() > 1000.0


class GroupStat:
    # An aggregate bucket: rolled up per region or per category.
    label: str
    orders: int
    units: int
    gross: float
    net: float

    def __init__(self, label: str) -> None:
        self.label = label
        self.orders = 0
        self.units = 0
        self.gross = 0.0
        self.net = 0.0

    def add(self, r: Record) -> None:
        self.orders = self.orders + 1
        self.units = self.units + r.units
        self.gross = self.gross + r.gross()
        self.net = self.net + r.net()

    def avg_order_net(self) -> float:
        if self.orders == 0:
            return 0.0
        return self.net / self.orders

    def discount_pct(self) -> float:
        # How much was discounted away, as a percentage of gross.
        if self.gross == 0.0:
            return 0.0
        return (1.0 - (self.net / self.gross)) * 100.0


# -----------------------------------------------------------------------------
# Embedded CSV data (the "file" we parse). Header + rows; some rows are
# deliberately malformed to exercise the cleaning path.
# -----------------------------------------------------------------------------

def raw_csv() -> str:
    # NOTE: pyrst's lexer rejects newlines inside string literals -- triple-quoted
    # multi-line strings are documented but not actually supported by the lexer.
    # So we assemble the CSV "file" from a list of row strings and join with "\n",
    # then parse it back out with splitlines() exactly as if it were a real file.
    rows: list[str] = [
        "order_id,region,category,units,unit_price,discount",
        "1001,North,Hardware,12,49.99,0.10",
        "1002,South,Software,3,199.00,0.00",
        "1003,North,Software,8,149.50,0.15",
        "1004,West,Hardware,25,49.99,0.20",
        "1005,East,Services,2,500.00,0.05",
        "1006,South,Hardware,40,19.95,0.25",
        "1007,West,Software,5,149.50,0.00",
        "1008,North,Services,1,500.00,0.00",
        "1009,East,Hardware,18,49.99,0.10",
        "1010,South,Software,7,199.00,0.30",
        "1011,west,services,9,75.00,0.00",
        "1012,North,Hardware,oops,49.99,0.10",
        "1013,East,Software,,149.50,0.05",
        "1014,South,Services,4,500.00,0.10",
        "1015,West,Hardware,30,19.95,0.15",
        "1016,malformed-row-too-few-fields",
        "1017,East,Software,6,149.50,0.20",
    ]
    return "\n".join(rows)


# -----------------------------------------------------------------------------
# Parsing / cleaning
# -----------------------------------------------------------------------------

def is_int_str(s: str) -> bool:
    # We MUST validate before calling int(): in pyrst, int("oops") lowers to a
    # Rust .parse().unwrap() that PANICS and aborts -- it does NOT raise a
    # catchable ValueError, so try/except cannot rescue it. Manual validation
    # is the only safe path.
    t: str = s.strip()
    if len(t) == 0:
        return False
    if t.startswith("-"):
        t = t[1:]
    if len(t) == 0:
        return False
    return t.isdigit()


def is_float_str(s: str) -> bool:
    # Same rationale as is_int_str: float("x") would panic, so we screen first.
    t: str = s.strip()
    if len(t) == 0:
        return False
    if t.startswith("-"):
        t = t[1:]
    if len(t) == 0:
        return False
    # Accept at most one decimal point; the rest must be digits.
    dots: int = t.count(".")
    if dots > 1:
        return False
    stripped: str = t.replace(".", "")
    if len(stripped) == 0:
        return False
    return stripped.isdigit()


def parse_records(text: str) -> list[Record]:
    records: list[Record] = []
    skipped: int = 0

    lines: list[str] = text.splitlines()
    # Drop the header row; comprehension over the remainder.
    body: list[str] = [ln for ln in lines[1:] if len(ln.strip()) > 0]

    for line in body:
        fields: list[str] = [f.strip() for f in line.split(",")]
        if len(fields) != 6:
            skipped = skipped + 1
            continue

        # Pre-validate every numeric field BEFORE parsing (see is_int_str).
        ok: bool = is_int_str(fields[0]) and is_int_str(fields[3])
        ok = ok and is_float_str(fields[4]) and is_float_str(fields[5])
        if not ok:
            skipped = skipped + 1
            continue

        oid: int = int(fields[0])
        region: str = normalize_region(fields[1])
        category: str = normalize_category(fields[2])
        units: int = int(fields[3])
        price: float = float(fields[4])
        disc: float = float(fields[5])

        if units <= 0 or price < 0.0:
            skipped = skipped + 1
            continue

        records.append(Record(oid, region, category, units, price, disc))

    print(f"parsed {len(records)} records, skipped {skipped} malformed")
    return records


def normalize_region(raw: str) -> str:
    # Title-case so "west" and "West" collapse to one group.
    return raw.strip().capitalize()


def normalize_category(raw: str) -> str:
    return raw.strip().capitalize()


# -----------------------------------------------------------------------------
# Aggregation
# -----------------------------------------------------------------------------

def group_key(r: Record, by_region: bool) -> str:
    # Pulled into its own function so the key is materialized as a fresh,
    # owned str (a return value) instead of moving a field out of `r` inside a
    # ternary -- the codegen otherwise partial-moves `r` and the borrow checker
    # rejects the later use of `r`.
    if by_region:
        return r.region
    return r.category


def new_group(label: str) -> GroupStat:
    # Thin wrapper around the GroupStat constructor. Calling it as a free
    # function makes the codegen clone the `label` argument, so the caller's
    # `key` variable survives -- whereas calling GroupStat(key) directly MOVES
    # `key` (constructor args are not auto-cloned the way function args are).
    return GroupStat(label)


def group_by(records: list[Record], by_region: bool) -> dict[str, GroupStat]:
    # NOTE: pyrst cannot mutate a dict value in place through a subscript
    # (d[k].add(r) would mutate a temporary). So we pull the GroupStat out,
    # mutate the local, and reassign the whole value back into the dict.
    groups: dict[str, GroupStat] = {}
    for r in records:
        key: str = group_key(r, by_region)
        # Avoid dict.get(key, GroupStat(key)): that lowering moves `key` into the
        # default constructor, then the later groups[key] = ... reuses it after
        # the move and the borrow checker rejects it. Membership-check instead so
        # the default bucket is built from a fresh key, not the loop variable.
        bucket: GroupStat = new_group(key)
        if key in groups:
            bucket = groups[key]
        bucket.add(r)
        groups[key] = bucket
    return groups


def global_summary(records: list[Record]) -> dict[str, float]:
    summary: dict[str, float] = {}
    nets: list[float] = [r.net() for r in records]
    summary["count"] = float(len(records))
    summary["total_net"] = sum(nets)
    summary["total_gross"] = sum([r.gross() for r in records])
    summary["avg_net"] = sum(nets) / len(nets)
    summary["max_net"] = max(nets)
    summary["min_net"] = min(nets)
    summary["total_units"] = float(sum([r.units for r in records]))
    return summary


# -----------------------------------------------------------------------------
# Reporting helpers
# -----------------------------------------------------------------------------

def money(x: float) -> str:
    # Fixed 2-decimal money string (no thousands separator: pyrst f-strings
    # lower to Rust format!, which has no {:,} grouping spec).
    return f"${x:.2f}"


def bar(pct: float, width: int) -> str:
    # A tiny text histogram bar scaled to `width`.
    filled: int = int((pct / 100.0) * width)
    if filled < 0:
        filled = 0
    if filled > width:
        filled = width
    out: str = ""
    for i in range(width):
        out = out + ("#" if i < filled else ".")
    return out


def ranked_labels(groups: dict[str, GroupStat]) -> list[str]:
    # Rank group labels by net revenue, descending.
    # We avoid combining key=lambda with reverse=True (the reverse kwarg is
    # ignored when a key is present), so: sort ascending by net, then reverse.
    stats: list[GroupStat] = [g for g in groups.values()]
    ordered: list[GroupStat] = sorted(stats, key=lambda g: g.net)
    ordered.reverse()
    return [g.label for g in ordered]


def print_group_table(title: str, groups: dict[str, GroupStat],
                      total_net: float) -> None:
    print("")
    print(f"=== {title} ===")
    header: str = f"{'GROUP':<10}{'ORDERS':>8}{'UNITS':>8}{'NET':>14}{'SHARE':>9}  CHART"
    print(header)
    print("-" * 64)

    for label in ranked_labels(groups):
        g: GroupStat = groups[label]
        share: float = (g.net / total_net) * 100.0 if total_net > 0.0 else 0.0
        net_str: str = money(g.net)
        chart: str = bar(share, 20)
        row: str = f"{g.label:<10}{g.orders:>8}{g.units:>8}{net_str:>14}{share:>8.1f}%  {chart}"
        print(row)


# -----------------------------------------------------------------------------
# Pipeline driver
# -----------------------------------------------------------------------------

def main() -> None:
    print("=== Sales Analytics Pipeline ===")

    records: list[Record] = parse_records(raw_csv())

    # -- Global summary ------------------------------------------------------
    summary: dict[str, float] = global_summary(records)
    print("")
    print("=== Global Summary ===")
    count_int: int = int(summary["count"])
    units_int: int = int(summary["total_units"])
    print(f"orders        : {count_int}")
    print(f"total units   : {units_int}")
    print(f"gross revenue : {money(summary['total_gross'])}")
    print(f"net revenue   : {money(summary['total_net'])}")
    print(f"avg net/order : {money(summary['avg_net'])}")
    print(f"max net order : {money(summary['max_net'])}")
    print(f"min net order : {money(summary['min_net'])}")

    total_net: float = summary["total_net"]

    # -- Group-by region -----------------------------------------------------
    by_region: dict[str, GroupStat] = group_by(records, True)
    print_group_table("Revenue by Region", by_region, total_net)

    # -- Group-by category ---------------------------------------------------
    by_category: dict[str, GroupStat] = group_by(records, False)
    print_group_table("Revenue by Category", by_category, total_net)

    # -- Discount analysis per category --------------------------------------
    print("")
    print("=== Discount Erosion by Category ===")
    cat_labels: list[str] = sorted([k for k in by_category.keys()])
    for label in cat_labels:
        g: GroupStat = by_category[label]
        erosion: float = g.discount_pct()
        lost: float = g.gross - g.net
        print(f"{g.label:<10} discount {erosion:>5.1f}%  lost {money(lost)}")

    # -- High-value order extraction (filter + transform) --------------------
    print("")
    print("=== High-Value Orders (net > $1000) ===")
    high: list[Record] = [r for r in records if r.is_high_value()]
    high_sorted: list[Record] = sorted(high, key=lambda r: r.net())
    high_sorted.reverse()
    for r in high_sorted:
        net_str: str = money(r.net())
        print(f"  #{r.order_id:>4}  {r.region:<6} {r.category:<10} {r.units:>3}u  {net_str:>12}")
    print(f"high-value count: {len(high_sorted)}")

    # -- Set-based diversity metrics -----------------------------------------
    regions_seen: set[str] = {r.region for r in records}
    cats_seen: set[str] = {r.category for r in records}
    print("")
    print("=== Diversity ===")
    print(f"distinct regions   : {len(regions_seen)}")
    print(f"distinct categories: {len(cats_seen)}")

    # -- Cross-tab: orders per (region) bucket via dict[str,int] -------------
    region_counts: dict[str, int] = {}
    for r in records:
        prev: int = region_counts.get(r.region, 0)
        region_counts[r.region] = prev + 1

    print("")
    print("=== Order Count by Region ===")
    for label in sorted([k for k in region_counts.keys()]):
        n: int = region_counts[label]
        print(f"{label:<10} {n:>3}  {('*' * n)}")

    # -- Threshold flags / boolean rollups -----------------------------------
    any_big: bool = any([r.units >= 30 for r in records])
    all_priced: bool = all([r.unit_price > 0.0 for r in records])
    print("")
    print("=== Data Quality Flags ===")
    print(f"has bulk order (>=30 units): {any_big}")
    print(f"all rows priced            : {all_priced}")

    # -- Leaderboard pick ----------------------------------------------------
    top_region: str = ranked_labels(by_region)[0]
    top_cat: str = ranked_labels(by_category)[0]
    print("")
    print("=== Leaders ===")
    print(f"top region   : {top_region}")
    print(f"top category : {top_cat}")
