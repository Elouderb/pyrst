def main() -> None:
    result: int  = 0
    try:
        result = (10 // 0)
    except:
        result = -1
    print(result)

