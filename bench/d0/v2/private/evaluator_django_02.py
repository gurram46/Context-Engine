import sys
sys.path.insert(0, '.')
from django.db.models import Q
q=Q(x=1)
p,_,_=q.deconstruct()
assert p=='django.db.models.Q', f'bad path {p}'
q2=Q(x=1, _connector='OR')
p2,_,k=q2.deconstruct()
assert k.get('_connector')=='OR', f'bad connector {k}'
print('pass')
