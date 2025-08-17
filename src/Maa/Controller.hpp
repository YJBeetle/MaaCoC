#pragma once

#include <MaaFramework/MaaAPI.h>

namespace Maa {
    class Controller {
    private:
        MaaController *controllerHandle = nullptr;

        static void NotifyForward(const char *message, const char *details_json, void *notify_trans_arg);

        void notify(const char *message, const char *details_json);

    public:
        Controller();

        ~Controller();

        auto handle() const { return controllerHandle; }

        bool setOption(MaaCtrlOption key, MaaOptionValue value, MaaOptionValueSize val_size) { return MaaControllerSetOption(controllerHandle, key, value, val_size); }

        MaaCtrlId postConnection() const { return MaaControllerPostConnection(controllerHandle); }

        MaaCtrlId postClick(int32_t x, int32_t y) const { return MaaControllerPostClick(controllerHandle, x, y); }
        MaaCtrlId postSwipe(int32_t x1, int32_t y1, int32_t x2, int32_t y2, int32_t duration) const { return MaaControllerPostSwipe(controllerHandle, x1, y1, x2, y2, duration); }
        MaaCtrlId postPressKey(int32_t keycode) const { return MaaControllerPostPressKey(controllerHandle, keycode); }
        MaaCtrlId postInputText(const char *text) const { return MaaControllerPostInputText(controllerHandle, text); }
        MaaCtrlId postStartApp(const char *intent) const { return MaaControllerPostStartApp(controllerHandle, intent); }
        MaaCtrlId postStopApp(const char *intent) const { return MaaControllerPostStopApp(controllerHandle, intent); }
        MaaCtrlId postTouchDown(int32_t contact, int32_t x, int32_t y, int32_t pressure) const { return MaaControllerPostTouchDown(controllerHandle, contact, x, y, pressure); }
        MaaCtrlId postTouchMove(int32_t contact, int32_t x, int32_t y, int32_t pressure) const { return MaaControllerPostTouchMove(controllerHandle, contact, x, y, pressure); }
        MaaCtrlId postTouchUp(int32_t contact) const { return MaaControllerPostTouchUp(controllerHandle, contact); }
        MaaCtrlId postScreencap() const { return MaaControllerPostScreencap(controllerHandle); }

        MaaStatus status(const MaaCtrlId id) const { return MaaControllerStatus(controllerHandle, id); }
        MaaStatus wait(const MaaCtrlId id) const { return MaaControllerWait(controllerHandle, id); }
        bool connected() const { return MaaControllerConnected(controllerHandle); }

        bool cachedImage(MaaImageBuffer *buffer) const { return MaaControllerCachedImage(controllerHandle, buffer); }
        bool getUuid(MaaStringBuffer *buffer) const { return MaaControllerGetUuid(controllerHandle, buffer); }
    };
}