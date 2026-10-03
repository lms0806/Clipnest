import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type SavedImage = {
  id: string;
  path: string;
  width: number;
  height: number;
  createdAt: string;
  bytes: number;
};

const gallery = document.querySelector<HTMLUListElement>("#gallery");
const empty = document.querySelector<HTMLParagraphElement>("#empty");
const count = document.querySelector<HTMLParagraphElement>("#count");
const status = document.querySelector<HTMLParagraphElement>("#status");

let images: SavedImage[] = [];
let copiedId = "";
let copiedTimer = 0;
let ready = false;
const pendingAdded: SavedImage[] = [];

function showStatus(message: string) {
  if (!status) {
    return;
  }
  status.hidden = false;
  status.textContent = message;
}

function errorMessage(error: unknown, fallback: string): string {
  if (typeof error === "string" && error.length > 0) {
    return error;
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return fallback;
}

function render() {
  if (!gallery || !empty || !count) {
    return;
  }

  count.textContent = `${images.length}개`;
  empty.hidden = images.length > 0;
  gallery.replaceChildren();

  for (const image of images) {
    const card = document.createElement("li");
    card.className = "card";
    card.dataset.id = image.id;
    if (image.id === copiedId) {
      card.classList.add("is-copied");
    }

    const thumb = document.createElement("button");
    thumb.type = "button";
    thumb.className = "thumb";
    thumb.addEventListener("click", () => {
      void copyImage(image.id);
    });

    const picture = document.createElement("img");
    picture.src = convertFileSrc(image.path);
    picture.alt = `저장된 이미지, ${image.width}×${image.height}`;
    picture.width = image.width;
    picture.height = image.height;
    picture.decoding = "async";

    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = "복사됨";

    thumb.append(picture, badge);

    const meta = document.createElement("div");
    meta.className = "meta";

    const size = document.createElement("span");
    size.className = "size";
    size.textContent = `${image.width}×${image.height}`;

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "delete";
    remove.textContent = "삭제";
    remove.addEventListener("click", () => {
      void removeImage(image.id);
    });

    meta.append(size, remove);
    card.append(thumb, meta);
    gallery.append(card);
  }
}

function addImage(image: SavedImage) {
  if (images.some((item) => item.id === image.id)) {
    return;
  }
  images = [image, ...images];
  render();
}

function markCopied(id: string) {
  copiedId = id;
  render();
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => {
    copiedId = "";
    render();
  }, 1600);
}

async function copyImage(id: string) {
  try {
    await invoke("copy_image", { id });
    markCopied(id);
  } catch (error) {
    showStatus(errorMessage(error, "복사하지 못했습니다"));
  }
}

async function removeImage(id: string) {
  try {
    await invoke("delete_image", { id });
  } catch (error) {
    showStatus(errorMessage(error, "삭제하지 못했습니다"));
  }
}

async function start() {
  if (!gallery) {
    return;
  }

  await listen<SavedImage>("image-added", (event) => {
    if (!ready) {
      pendingAdded.push(event.payload);
      return;
    }
    addImage(event.payload);
  });

  await listen<string>("image-removed", (event) => {
    images = images.filter((image) => image.id !== event.payload);
    render();
  });

  try {
    images = await invoke<SavedImage[]>("list_images");
    render();
  } catch (error) {
    showStatus(errorMessage(error, "저장 목록을 불러오지 못했습니다"));
  }

  ready = true;
  for (const image of pendingAdded) {
    addImage(image);
  }
  pendingAdded.length = 0;
}

void start();
