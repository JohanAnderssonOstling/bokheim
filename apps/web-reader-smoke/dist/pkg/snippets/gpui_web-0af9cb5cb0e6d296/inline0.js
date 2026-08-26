
export function gpuiPromptForFiles(accept, multiple) {
    return new Promise((resolve, reject) => {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = accept;
        input.multiple = multiple;
        input.style.display = "none";
        document.body.appendChild(input);

        let settled = false;
        const finish = (value) => {
            if (settled) return;
            settled = true;
            input.remove();
            resolve(value);
        };
        input.addEventListener("cancel", () => finish(null), { once: true });
        input.addEventListener("change", async () => {
            try {
                const selected = await Promise.all(Array.from(input.files ?? []).map(async file => ({
                    name: file.name,
                    bytes: new Uint8Array(await file.arrayBuffer()),
                })));
                finish(selected);
            } catch (error) {
                input.remove();
                reject(error);
            }
        }, { once: true });

        try {
            if (typeof input.showPicker === "function") input.showPicker();
            else input.click();
        } catch (error) {
            input.remove();
            reject(error);
        }
    });
}
