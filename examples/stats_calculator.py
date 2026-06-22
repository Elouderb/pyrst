# Statistical calculator with data processing

class DataSet:
    name: str
    values: list[float]

    def __init__(self, name: str) -> None:
        self.name = name
        self.values = []

# `add_value` mutates the caller's DataSet in place (it appends to ds.values),
# so `ds` is declared Mut[DataSet] — a by-reference param whose mutation IS
# visible to the caller (EPIC-4 V2). Passing it by value would lose every
# append on a clone (a silent wrong-output bug the V2-d backstop now rejects);
# the read-only accessors below keep their by-value `DataSet` params.
def add_value(ds: Mut[DataSet], val: float) -> None:
    ds.values.append(val)

def get_mean(ds: DataSet) -> float:
    if len(ds.values) == 0:
        return 0.0
    total = 0.0
    for v in ds.values:
        total = total + v
    return total / len(ds.values)

def get_median(ds: DataSet) -> float:
    if len(ds.values) == 0:
        return 0.0
    sorted_vals = sorted(ds.values)
    n = len(sorted_vals)
    if n % 2 == 0:
        mid1 = sorted_vals[n // 2 - 1]
        mid2 = sorted_vals[n // 2]
        return (mid1 + mid2) / 2.0
    else:
        return sorted_vals[n // 2]

def get_range(ds: DataSet) -> float:
    if len(ds.values) == 0:
        return 0.0
    max_val = ds.values[0]
    min_val = ds.values[0]
    for v in ds.values:
        if v > max_val:
            max_val = v
        if v < min_val:
            min_val = v
    return max_val - min_val

def get_variance(ds: DataSet) -> float:
    if len(ds.values) == 0:
        return 0.0
    mean = get_mean(ds)
    sum_sq_diff = 0.0
    for v in ds.values:
        diff = v - mean
        sum_sq_diff = sum_sq_diff + (diff * diff)
    return sum_sq_diff / len(ds.values)

def main() -> None:
    # Create first dataset
    ds1 = DataSet("Dataset1")
    add_value(ds1, 10.0)
    add_value(ds1, 20.0)
    add_value(ds1, 30.0)
    add_value(ds1, 40.0)
    add_value(ds1, 50.0)

    print("=== Dataset1 Statistics ===")
    print(get_mean(ds1))
    print(get_median(ds1))
    print(get_range(ds1))
    print(get_variance(ds1))

    # Create second dataset
    ds2 = DataSet("Dataset2")
    add_value(ds2, 5.0)
    add_value(ds2, 15.0)
    add_value(ds2, 25.0)
    add_value(ds2, 35.0)

    print("=== Dataset2 Statistics ===")
    print(get_mean(ds2))
    print(get_median(ds2))
    print(get_range(ds2))
    print(get_variance(ds2))

    # Compare datasets
    print("=== Comparison ===")
    mean1 = get_mean(ds1)
    mean2 = get_mean(ds2)
    mean_diff = mean1 - mean2
    print(mean_diff)

    var1 = get_variance(ds1)
    var2 = get_variance(ds2)
    var_diff = var1 - var2
    print(var_diff)

    # Summary
    print("=== Summary ===")
    print(len(ds1.values))
    print(len(ds2.values))
    total_values = len(ds1.values) + len(ds2.values)
    print(total_values)
