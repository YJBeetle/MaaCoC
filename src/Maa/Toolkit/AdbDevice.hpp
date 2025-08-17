#pragma once

#include <MaaToolkit/MaaToolkitAPI.h>

namespace Maa::Toolkit {
    class AdbDevice {
    private:
        MaaToolkitAdbDeviceList *listHandle = nullptr;
        const MaaToolkitAdbDevice *deviceHandle = nullptr;

    public:
        AdbDevice();

        ~AdbDevice();

        void selectDeviceInteractive();

        const char *name() const { return deviceHandle ? MaaToolkitAdbDeviceGetName(deviceHandle) : nullptr; }
        const char *adbPath() const { return deviceHandle ? MaaToolkitAdbDeviceGetAdbPath(deviceHandle) : nullptr; }
        const char *address() const { return deviceHandle ? MaaToolkitAdbDeviceGetAddress(deviceHandle) : nullptr; }
        MaaAdbScreencapMethod screencapMethods() const { return deviceHandle ? MaaToolkitAdbDeviceGetScreencapMethods(deviceHandle) : MaaAdbScreencapMethod_None; }
        MaaAdbInputMethod inputMethods() const { return deviceHandle ? MaaToolkitAdbDeviceGetInputMethods(deviceHandle) : MaaAdbInputMethod_None; }
        const char *config() const { return deviceHandle ? MaaToolkitAdbDeviceGetConfig(deviceHandle) : nullptr; }
    };
}
