type Props = {
  title: string;
  note: string;
};

export function PageStub({ title, note }: Props) {
  return (
    <div className="flex flex-col gap-2 p-6">
      <h1 className="m-0 text-lg font-semibold">{title}</h1>
      <p className="m-0 text-sm text-muted-foreground">{note}</p>
    </div>
  );
}
