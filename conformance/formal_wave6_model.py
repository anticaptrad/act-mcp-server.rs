#!/usr/bin/env python3
from itertools import product
for auth,tenant,mutating,write in product((False,True),repeat=4):
  admitted=auth and tenant and (not mutating or write)
  if admitted and mutating: assert write
  if auth and tenant and not mutating: assert admitted
  if not auth or not tenant: assert not admitted
print("tool authority lattice: ok")
