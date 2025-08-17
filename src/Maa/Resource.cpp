#include "../Log.h"
#include "./Resource.hpp"

namespace Maa {
    void Resource::NotifyForward(const char *message, const char *details_json, void *notify_trans_arg) {
        auto *self = static_cast<Resource *>(notify_trans_arg);
        self->notify(message, details_json);
    }

    void Resource::notify(const char *message, const char *details_json) {
        LOG_INFO("Resource", "Notify received: %s, details: %s", message, details_json);
    }

    Resource::Resource() {
        resourceHandle = MaaResourceCreate(NotifyForward, this);
    }

    Resource::~Resource() {
        MaaResourceDestroy(resourceHandle);
    }
}
