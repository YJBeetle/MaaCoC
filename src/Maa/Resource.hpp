#pragma once

#include <MaaFramework/MaaAPI.h>

namespace Maa {
    class Resource {
    private:
        MaaResource *resourceHandle = nullptr;

        static void NotifyForward(const char *message, const char *details_json, void *notify_trans_arg);

        void notify(const char *message, const char *details_json);

    public:
        Resource();

        ~Resource();

        auto handle() const { return resourceHandle; }

        MaaBool registerCustomRecognition(const char *name, MaaCustomRecognitionCallback recognition, void *trans_arg) const { return MaaResourceRegisterCustomRecognition(resourceHandle, name, recognition, trans_arg); }
        MaaBool unregisterCustomRecognition(const char *name) const { return MaaResourceUnregisterCustomRecognition(resourceHandle, name); }
        MaaBool clearCustomRecognition() const { return MaaResourceClearCustomRecognition(resourceHandle); }

        MaaBool registerCustomAction(const char *name, MaaCustomActionCallback action, void *trans_arg) const { return MaaResourceRegisterCustomAction(resourceHandle, name, action, trans_arg); }
        MaaBool unregisterCustomAction(const char *name) const { return MaaResourceUnregisterCustomAction(resourceHandle, name); }
        MaaBool clearCustomAction() const { return MaaResourceClearCustomAction(resourceHandle); }

        MaaResId postBundle(const char *path) const { return MaaResourcePostBundle(resourceHandle, path); }

        MaaBool overridePipeline(const char *pipeline_override) const { return MaaResourceOverridePipeline(resourceHandle, pipeline_override); }
        MaaBool overrideNext(const char *node_name, const MaaStringListBuffer *next_list) const { return MaaResourceOverrideNext(resourceHandle, node_name, next_list); }

        MaaBool getNodeData(const char *node_name, MaaStringBuffer *buffer) const { return MaaResourceGetNodeData(resourceHandle, node_name, buffer); }

        MaaBool clear() const { return MaaResourceClear(resourceHandle); }
        MaaStatus status(const MaaResId id) const { return MaaResourceStatus(resourceHandle, id); }
        MaaStatus wait(const MaaResId id) const { return MaaResourceWait(resourceHandle, id); }
        MaaBool loaded() const { return MaaResourceLoaded(resourceHandle); }

        MaaBool setOption(MaaResOption key, MaaOptionValue value, MaaOptionValueSize val_size) const { return MaaResourceSetOption(resourceHandle, key, value, val_size); }

        MaaBool getHash(MaaStringBuffer *buffer) const { return MaaResourceGetHash(resourceHandle, buffer); }
        MaaBool getNodeList(MaaStringListBuffer *buffer) const { return MaaResourceGetNodeList(resourceHandle, buffer); }
    };
}
