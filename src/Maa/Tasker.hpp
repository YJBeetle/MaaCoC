#pragma once

#include <MaaFramework/MaaAPI.h>

namespace Maa {
    class Tasker {
    private:
        MaaTasker *taskerHandle = nullptr;

        static void NotifyForward(const char *message, const char *details_json, void *notify_trans_arg);

        void notify(const char *message, const char *details_json);

    public:
        Tasker();

        Tasker(const class Controller &controller, const class Resource &resource);

        ~Tasker();

        auto handle() const { return taskerHandle; }

        bool setOption(MaaTaskerOption key, MaaOptionValue value, MaaOptionValueSize val_size) { return MaaTaskerSetOption(taskerHandle, key, value, val_size); }

        bool bindResource(MaaResource *res) { return MaaTaskerBindResource(taskerHandle, res); }
        bool bindController(MaaController *ctrl) { return MaaTaskerBindController(taskerHandle, ctrl); }
        bool inited() const { return MaaTaskerInited(taskerHandle); }

        MaaTaskId postTask(const char *entry, const char *pipelineOverride) const { return MaaTaskerPostTask(taskerHandle, entry, pipelineOverride); }
        MaaStatus status(const MaaTaskId id) const { return MaaTaskerStatus(taskerHandle, id); }
        MaaStatus wait(const MaaTaskId id) const { return MaaTaskerWait(taskerHandle, id); }

        bool running() const { return MaaTaskerRunning(taskerHandle); }
        MaaTaskId postStop() const { return MaaTaskerPostStop(taskerHandle); }
        bool stopping() const { return MaaTaskerStopping(taskerHandle); }
        bool clearCache() { return MaaTaskerClearCache(taskerHandle); }

        bool getRecognitionDetail(
            MaaRecoId reco_id,
            MaaStringBuffer *node_name,
            MaaStringBuffer *algorithm,
            MaaBool *hit,
            MaaRect *box,
            MaaStringBuffer *detail_json,
            MaaImageBuffer *raw,
            MaaImageListBuffer *draws) const {
            return MaaTaskerGetRecognitionDetail(taskerHandle, reco_id, node_name, algorithm, hit, box, detail_json, raw, draws);
        }

        bool getNodeDetail(
            MaaNodeId node_id,
            MaaStringBuffer *node_name,
            MaaRecoId *reco_id,
            MaaBool *completed) const {
            return MaaTaskerGetNodeDetail(taskerHandle, node_id, node_name, reco_id, completed);
        }

        bool getTaskDetail(
            MaaTaskId task_id,
            MaaStringBuffer *entry,
            MaaNodeId *node_id_list,
            MaaSize *node_id_list_size,
            MaaStatus *status) const {
            return MaaTaskerGetTaskDetail(taskerHandle, task_id, entry, node_id_list, node_id_list_size, status);
        }

        bool getLatestNode(const char *node_name, MaaNodeId *latest_id) const {
            return MaaTaskerGetLatestNode(taskerHandle, node_name, latest_id);
        }
    };
}
