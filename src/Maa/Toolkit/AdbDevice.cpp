#include <iostream>
#include "../../Log.h"
#include "./AdbDevice.hpp"

namespace Maa::Toolkit {
    AdbDevice::AdbDevice() {
        listHandle = MaaToolkitAdbDeviceListCreate();
        MaaToolkitAdbDeviceFind(listHandle);
    }

    AdbDevice::~AdbDevice() {
        MaaToolkitAdbDeviceListDestroy(listHandle);
    }

    void AdbDevice::selectDeviceInteractive() {
        size_t size = MaaToolkitAdbDeviceListSize(listHandle);
        if (size == 0) {
            LOG_ERROR("AdbDevice", "No ADB device found");
            throw std::runtime_error("No ADB device found");
        }
        LOG_DEBUG("AdbDevice", "Found %zu ADB devices", size);

        size_t kIndex = 0;
        if (size > 1) {
            std::cout << "ADB device list:\n";
            for (size_t i = 0; i < size; i++) {
                auto device_handle = MaaToolkitAdbDeviceListAt(listHandle, i);
                std::cout << "\t" << i << ". " << MaaToolkitAdbDeviceGetAddress(device_handle) << std::endl;
            }
            std::cout << "Enter the index of the device you want to use (default: " << kIndex << "): ";
            std::string input;
            std::getline(std::cin, input);
            if (!input.empty()) {
                kIndex = std::stoi(input);
            }
        }
        LOG_INFO("AdbDevice", "Using ADB device at index: %zu", kIndex);
        if (kIndex < 0 || kIndex >= size) {
            LOG_ERROR("AdbDevice", "Invalid index: %zu", kIndex);
            throw std::runtime_error("Invalid index");
        }
        deviceHandle = MaaToolkitAdbDeviceListAt(listHandle, kIndex);
    }
}
