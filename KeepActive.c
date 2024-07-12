#include <stdio.h>
#include <windows.h>
#include <stdbool.h>
#include <pthread.h>
#include <stdatomic.h>
#include <unistd.h>
#include <string.h>
#include <wchar.h>

atomic_bool isActive = false;
wchar_t windowName[256] = L"CounterSide";

void* keepActive(void* arg) {
    while (atomic_load(&isActive)) {
        HWND hwnd = FindWindowW(NULL, windowName);
        if (hwnd != NULL) {
            SendMessageW(hwnd, WM_ACTIVATE, WA_CLICKACTIVE, 0);
        }
        usleep(100000);
    }
    return NULL;
}

int main(int argc, char* argv[]) {
    for (int i = 1; i < argc - 1; i++) {
        if (strcmp(argv[i], "-w") == 0) {
            mbstowcs(windowName, argv[i+1], strlen(argv[i+1]) + 1);
            break;
        }
    }

    printf("Keep Active - C CLI Version\n");
    printf("Window name: %ls\n", windowName);
    printf("Type '1' to turn on, '0' to turn off, 'q' to quit\n");

    pthread_t keepActiveThread;
    bool threadRunning = false;

    char input;
    while (true) {
        scanf(" %c", &input);

        if (input == '1' && !atomic_load(&isActive)) {
            atomic_store(&isActive, true);
            if (pthread_create(&keepActiveThread, NULL, keepActive, NULL) != 0) {
                fprintf(stderr, "Error creating thread\n");
                return 1;
            }
            threadRunning = true;
            printf("Running\n");
        } else if (input == '0' && atomic_load(&isActive)) {
            atomic_store(&isActive, false);
            if (threadRunning) {
                pthread_join(keepActiveThread, NULL);
                threadRunning = false;
            }
            printf("Not Running\n");
        } else if (input == 'q') {
            if (atomic_load(&isActive)) {
                atomic_store(&isActive, false);
                if (threadRunning) {
                    pthread_join(keepActiveThread, NULL);
                    threadRunning = false;
                }
            }
            break;
        }
    }

    printf("Exiting program\n");
    return 0;
}