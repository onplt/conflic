FROM mcr.microsoft.com/dotnet/sdk:9.0 AS build
WORKDIR /app
COPY . .
RUN dotnet publish -c Release

FROM mcr.microsoft.com/dotnet/aspnet:9.0
COPY --from=build /app/bin/Release/net8.0/publish .
ENTRYPOINT ["dotnet", "MyApp.dll"]
