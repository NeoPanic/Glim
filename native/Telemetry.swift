import Foundation
import Darwin
import Metal

// Own-process counters only. No account, hardware serial, process command line
// or document-derived data crosses this fixed numeric ABI. Safe off main thread.
@_cdecl("glim_telemetry_resources")
func glimTelemetryResources(_ output: UnsafeMutablePointer<UInt64>, _ capacity: Int) -> Int32 {
    guard capacity >= 16 else { return 0 }
    for index in 0..<16 { output[index] = 0 }
    var vm = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let status = withUnsafeMutablePointer(to: &vm) { pointer in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    var available: Int32 = 0
    if status == KERN_SUCCESS {
        output[0] = vm.resident_size
        output[1] = vm.phys_footprint
        output[2] = vm.virtual_size
        output[3] = vm.compressed
        available |= 1
    }
    var usage = rusage()
    if getrusage(RUSAGE_SELF, &usage) == 0 {
        let user = UInt64(max(0, usage.ru_utime.tv_sec)) * 1_000_000_000 + UInt64(max(0, usage.ru_utime.tv_usec)) * 1_000
        let system = UInt64(max(0, usage.ru_stime.tv_sec)) * 1_000_000_000 + UInt64(max(0, usage.ru_stime.tv_usec)) * 1_000
        output[4] = user + system
        output[5] = UInt64(max(0, usage.ru_maxrss)) // Darwin reports bytes.
        available |= 2
    }
    let process = ProcessInfo.processInfo
    output[6] = process.physicalMemory
    output[7] = UInt64(process.processorCount)
    output[8] = UInt64(process.activeProcessorCount)
    output[9] = UInt64(max(0, process.systemUptime))
    var devices = Set<UInt64>()
    for device in MTLCopyAllDevices() where devices.insert(device.registryID).inserted {
        output[10] += UInt64(device.currentAllocatedSize)
        if device.hasUnifiedMemory { output[11] = 1 }
        available |= 4
    }
    switch process.thermalState {
    case .nominal: output[12] = 0
    case .fair: output[12] = 1
    case .serious: output[12] = 2
    case .critical: output[12] = 3
    @unknown default: output[12] = 4
    }
    if #available(macOS 12.0, *) { output[13] = process.isLowPowerModeEnabled ? 1 : 0; available |= 8 }
    output[14] = UInt64(devices.count)
    var translated: Int32 = 0
    var translatedSize = MemoryLayout<Int32>.size
    if sysctlbyname("sysctl.proc_translated", &translated, &translatedSize, nil, 0) == 0 {
        output[15] = translated == 1 ? 1 : 0; available |= 16
    }
    return available
}
