def is_success_code(code: int) -> bool:
    success_codes: set[int] = {200, 201, 202, 204}
    return code in success_codes

def is_client_error(code: int) -> bool:
    client_errors: set[int] = {400, 401, 403, 404}
    return code in client_errors

def main() -> None:
    code1: int = 200
    code2: int = 404
    code3: int = 500

    if is_success_code(code1):
        print(1)
    else:
        print(0)

    if is_client_error(code2):
        print(1)
    else:
        print(0)

    status_set: set[int] = {200, 404, 500}
    test_code: int = 200
    if test_code in status_set:
        print(1)
    else:
        print(0)
