# Matrix-like operations with nested lists

def main() -> None:
    # 3x3 matrix as nested lists
    matrix = [
        [1.0, 2.0, 3.0],
        [4.0, 5.0, 6.0],
        [7.0, 8.0, 9.0],
    ]

    print("=== Matrix Operations ===")

    # Sum all elements
    total = 0.0
    for row in matrix:
        for val in row:
            total = total + val
    print(total)

    # Count elements
    count = 0
    for row in matrix:
        count = count + len(row)
    print(count)

    # Find max and min
    max_val = matrix[0][0]
    min_val = matrix[0][0]
    for row in matrix:
        for val in row:
            if val > max_val:
                max_val = val
            if val < min_val:
                min_val = val

    print(max_val)
    print(min_val)

    # Row sums
    print("=== Row Sums ===")
    row_sum_1 = 0.0
    for val in matrix[0]:
        row_sum_1 = row_sum_1 + val
    print(row_sum_1)

    row_sum_2 = 0.0
    for val in matrix[1]:
        row_sum_2 = row_sum_2 + val
    print(row_sum_2)

    row_sum_3 = 0.0
    for val in matrix[2]:
        row_sum_3 = row_sum_3 + val
    print(row_sum_3)

    # Column sums
    print("=== Column Sums ===")
    col_sum_1 = matrix[0][0] + matrix[1][0] + matrix[2][0]
    col_sum_2 = matrix[0][1] + matrix[1][1] + matrix[2][1]
    col_sum_3 = matrix[0][2] + matrix[1][2] + matrix[2][2]

    print(col_sum_1)
    print(col_sum_2)
    print(col_sum_3)

    # Diagonal sum (top-left to bottom-right)
    diag_sum = matrix[0][0] + matrix[1][1] + matrix[2][2]
    print(diag_sum)

    # Transpose-like operation
    print("=== Transpose ===")
    trans_1_1 = matrix[0][0]
    trans_1_2 = matrix[1][0]
    trans_1_3 = matrix[2][0]
    print(trans_1_1)
    print(trans_1_2)
    print(trans_1_3)

    # Average
    avg = total / 9.0
    print(avg)

    # Element-wise operations
    print("=== Scaled ===")
    scaled_val = matrix[0][0] * 2.0
    print(scaled_val)

    scaled_sum = 0.0
    for row in matrix:
        for val in row:
            scaled_sum = scaled_sum + (val * 2.0)
    print(scaled_sum)
