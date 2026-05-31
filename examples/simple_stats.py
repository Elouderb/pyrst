# Simple statistics - working within current limitations

def main() -> None:
    # Test data as separate variables
    vals = [10.0, 20.0, 30.0, 40.0, 50.0]

    print("=== Simple Statistics ===")
    print(len(vals))

    # Calculate sum
    total = 0.0
    for v in vals:
        total = total + v
    print(total)

    # Calculate average
    count = 5.0
    avg = total / count
    print(avg)

    # Find max and min
    max_val = vals[0]
    min_val = vals[0]
    for v in vals:
        if v > max_val:
            max_val = v
        if v < min_val:
            min_val = v

    print(max_val)
    print(min_val)
    print(max_val - min_val)

    # Count values above average
    above_avg = 0
    below_avg = 0
    for v in vals:
        if v > avg:
            above_avg = above_avg + 1
        else:
            below_avg = below_avg + 1

    print(above_avg)
    print(below_avg)

    # Squared deviations
    sum_sq_dev = 0.0
    for v in vals:
        dev = v - avg
        sum_sq_dev = sum_sq_dev + (dev * dev)

    variance = sum_sq_dev / count
    print(variance)

    # Percentiles
    percentile_20 = vals[0]
    percentile_80 = vals[4]
    print(percentile_20)
    print(percentile_80)
