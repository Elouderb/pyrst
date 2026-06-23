def parse_config_line(line: str) -> dict[str, str]:
    result = {}

    if line.find("=") > 0:
        parts = line.split("=")
        if len(parts) == 2:
            key = parts[0].strip()
            value = parts[1].strip()
            result["key"] = key
            result["value"] = value

    return result

def main() -> None:
    # Configuration lines
    config_lines = [
        "host = localhost",
        "port = 8080",
        "debug = true",
        "timeout = 30",
        "max_connections = 100",
        "database = postgres",
        "ssl_enabled = false",
        "log_level = INFO",
    ]

    print("=== Configuration Parser ===")
    print(len(config_lines))

    # Parse all lines
    configs = {}
    for line in config_lines:
        parsed = parse_config_line(line)
        if "key" in parsed:
            key = parsed["key"]
            value = parsed["value"]
            configs[key] = value

    print(len(configs))

    # Extract specific values
    host = configs["host"]
    port_str = configs["port"]
    port = int(port_str)

    print(host)
    print(port)

    # Boolean parsing
    debug_str = configs["debug"]
    ssl_str = configs["ssl_enabled"]

    debug_enabled = debug_str.lower() == "true"
    ssl_enabled = ssl_str.lower() == "true"

    print(debug_enabled)
    print(ssl_enabled)

    # Numeric values
    timeout_str = configs["timeout"]
    max_conn_str = configs["max_connections"]

    timeout = int(timeout_str)
    max_conn = int(max_conn_str)

    print(timeout)
    print(max_conn)

    # String configuration
    database = configs["database"]
    log_level = configs["log_level"]

    print(database)
    print(log_level)

    # Config validation
    config_valid = True
    if port < 1 or port > 65535:
        config_valid = False
    if timeout < 1:
        config_valid = False
    if max_conn < 1:
        config_valid = False

    print(config_valid)

    # Key analysis
    keys = ["host", "port", "debug", "timeout", "max_connections", "database", "ssl_enabled", "log_level"]
    parsed_keys = []
    for key in keys:
        if key in configs:
            parsed_keys.append(key)

    print(len(parsed_keys))

    # Values as strings
    all_values = ["localhost", port_str, debug_str, timeout_str, max_conn_str, database, ssl_str, log_level]
    print(len(all_values))

    # Connection string builder
    conn_string = ""
    conn_string = "host=" + host
    conn_string = conn_string + " port=" + port_str
    conn_string = conn_string + " database=" + database

    print(conn_string)

    # Security check
    has_ssl = ssl_enabled
    is_production = debug_enabled == False

    print(has_ssl)
    print(is_production)

    # Advanced: check for required keys
    required_keys = ["host", "port", "database"]
    all_required = True
    for req_key in required_keys:
        if req_key not in configs:
            all_required = False

    print(all_required)
