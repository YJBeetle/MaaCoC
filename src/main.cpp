#include <array>
#include <iostream>

#include <MaaToolkit/MaaToolkitAPI.h>

#include "./Log.h"
#include "./Logo.hpp"
#include "./Maa/Controller.hpp"
#include "./Maa/Resource.hpp"
#include "./Maa/Tasker.hpp"

int main([[maybe_unused]] int argc, char **argv) {
    std::cout << LOGO;

    LOG_INFO("main", "MaaFramework Version: %s", MaaVersion());

    std::string user_path = "./";
    LOG_DEBUG("main", "User path: %s", user_path.c_str());
    MaaToolkitConfigInitOption(user_path.c_str(), "{}");

    try {
        LOG_INFO("main", "Creating Controller...");
        Maa::Controller controller;
        auto ctrlId = controller.postConnection();

        LOG_INFO("main", "Creating Resource...");
        auto resource = Maa::Resource();
        std::string resourceDir = R"(./assets)";
        LOG_DEBUG("main", "Resource directory: %s", resourceDir.c_str());
        auto resId = resource.postBundle(resourceDir.c_str());

        LOG_INFO("main", "Waiting for Controller and Resource ready...");
        controller.wait(ctrlId);
        resource.wait(resId);

        LOG_INFO("main", "Creating Tasker...");
        Maa::Tasker tasker(controller, resource);

        LOG_INFO("main", "Starting MaaFramework MainTask...");
        auto taskId = tasker.postTask("Main", "{}");
        tasker.wait(taskId);
    } catch (const std::runtime_error &e) {
        LOG_FATAL("main", "Runtime error: %s", e.what());
        return 1;
    }

    return 0;
}
