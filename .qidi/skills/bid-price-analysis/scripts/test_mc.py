# -*- coding: utf-8 -*-
import io, sys, json
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from hefei_price import monte_carlo_optimal
r = monte_carlo_optimal(10996081.66, [0.88, 0.90, 0.92, 0.93, 0.95, 0.97])
print(json.dumps(r, ensure_ascii=False, indent=1, default=str)[:800])
