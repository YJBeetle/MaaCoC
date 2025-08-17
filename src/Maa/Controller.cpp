#include "../Log.h"
#include "./Toolkit/AdbDevice.hpp"
#include "./Controller.hpp"

#include <string>
#include <stdexcept>

namespace Maa {
    void Controller::NotifyForward(const char *message, const char *details_json, void *notify_trans_arg) {
        auto *self = static_cast<Controller *>(notify_trans_arg);
        self->notify(message, details_json);
    }

    void Controller::notify(const char *message, const char *details_json) {
        LOG_INFO("Controller", "Notify received: %s, details: %s", message, details_json);
    }

    Controller::Controller() {
        Toolkit::AdbDevice adbDevice;
        adbDevice.selectDeviceInteractive();
        std::string agentPath = "MaaAgentBinary";
        controllerHandle = MaaAdbControllerCreate(
            adbDevice.adbPath(),
            adbDevice.address(),
            adbDevice.screencapMethods(),
            adbDevice.inputMethods(),
            adbDevice.config(),
            agentPath.c_str(),
            NotifyForward, this);
        if (controllerHandle == nullptr) {
            LOG_ERROR("Controller", "Failed to create MaaController");
            throw std::runtime_error("Failed to create MaaController");
        }
    }

    Controller::~Controller() {
        MaaControllerDestroy(controllerHandle);
    }
}
