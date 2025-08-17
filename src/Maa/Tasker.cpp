#include <functional>
#include <stdexcept>
#include "../Log.h"
#include "./Controller.hpp"
#include "./Resource.hpp"
#include "./Tasker.hpp"

namespace Maa {
    void Tasker::NotifyForward(const char *message, const char *details_json, void *notify_trans_arg) {
        auto *self = static_cast<Tasker *>(notify_trans_arg);
        self->notify(message, details_json);
    }

    void Tasker::notify(const char *message, const char *details_json) {
        LOG_INFO("Tasker", "Notify received: %s, details: %s", message, details_json);
    }

    Tasker::Tasker() {
        taskerHandle = MaaTaskerCreate(NotifyForward, this);
    }

    Tasker::Tasker(const Controller &controller, const Resource &resource) : Tasker() {
        bindResource(resource.handle());
        bindController(controller.handle());
        if (!inited()) {
            LOG_ERROR("Tasker", "Failed to init MaaTasker");
            throw std::runtime_error("Failed to init MaaTasker");
        }
    }

    Tasker::~Tasker() {
        MaaTaskerDestroy(taskerHandle);
    }
}
