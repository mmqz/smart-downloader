from ghidra.app.decompiler import DecompInterface
from ghidra.util.task import ConsoleTaskMonitor
import java.io.File as File

decomp = DecompInterface()
decomp.openProgram(currentProgram)
monitor = ConsoleTaskMonitor()

addrs = [0x180161920, 0x180162190, 0x180159610, 0x1801595f0, 0x180164970]

for addr_val in addrs:
    addr = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(addr_val)
    func = getFunctionAt(addr)
    if func is None:
        func = createFunction(addr, "func_%x" % addr_val)
    if func:
        result = decomp.decompileFunction(func, 120, monitor)
        if result.decompileCompleted():
            print("=== FUNCTION @ 0x%x ===" % addr_val)
            print(result.getDecompiledFunction().getC())
            print("=== END ===\n")
        else:
            print("DECOMPILE FAILED @ 0x%x: %s" % (addr_val, str(result.getErrorMessage())))
    else:
        print("NO FUNCTION @ 0x%x" % addr_val)

decomp.dispose()
