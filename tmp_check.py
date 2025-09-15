import sys, importlib.util
sys.path.insert(0, r'C:\Users\sande\OneDrive\Desktop\Context Engine')
import context_engine
print('pkg:', context_engine.__file__)
print('find ui:', importlib.util.find_spec('context_engine.ui') is not None)
