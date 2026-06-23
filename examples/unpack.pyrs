def minmax(nums: list[int]) -> tuple[int, int]:
    mn: int  = nums[0]
    mx: int  = nums[0]
    for n in nums:
        if (n < mn):
            mn = n
        if (n > mx):
            mx = n
    return (mn, mx)


def main() -> None:
    (lo, hi) = minmax([3, 1, 4, 1, 5, 9, 2, 6])
    print(lo)
    print(hi)

